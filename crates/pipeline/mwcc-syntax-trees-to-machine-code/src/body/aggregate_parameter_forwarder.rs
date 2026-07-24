//! Aggregate-local wrappers that forward entry parameters to one helper.
//!
//! Parameters beyond r10 live in the caller's linkage area. This owner loads
//! those values before populating the local aggregate and schedules the two
//! outgoing call arguments among the independent stores.

#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy)]
enum StoreSource {
    General(u8),
    Float(u8),
}

#[derive(Clone, Copy)]
struct PlannedStore {
    source: StoreSource,
    offset: i16,
    width: u8,
}

impl Generator {
    pub(crate) fn try_aggregate_parameter_forwarder(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if function.return_type != Type::Void
            || function.return_expression.is_some()
            || !function.guards.is_empty()
            || function.parameters.len() < 9
        {
            return Ok(false);
        }
        let [local] = function.locals.as_slice() else {
            return Ok(false);
        };
        let Type::Struct { size, align } = local.declared_type else {
            return Ok(false);
        };
        if local.is_static
            || local.array_length.is_some()
            || local.initializer.is_some()
            || local.data_bytes.is_some()
            || align > 4
        {
            return Ok(false);
        }
        let Some((call_statement, store_statements)) = function.statements.split_last() else {
            return Ok(false);
        };
        if store_statements.len() + 1 != function.parameters.len() {
            return Ok(false);
        }
        let Statement::Expression(Expression::Call {
            name: callee,
            arguments,
        }) = call_statement
        else {
            return Ok(false);
        };
        let [Expression::AddressOf { operand }, Expression::Variable(forwarded)] =
            arguments.as_slice()
        else {
            return Ok(false);
        };
        if !matches!(operand.as_ref(), Expression::Variable(name) if name == &local.name)
            || forwarded != &function.parameters[0].name
            || self.locations.contains_key(callee)
            || self.globals.contains_key(callee)
        {
            return Ok(false);
        }

        let Ok(payload_size) = i16::try_from(size) else {
            return Ok(false);
        };
        let Some(frame_size) = payload_size
            .checked_add(8)
            .and_then(|size| size.checked_add(15))
            .map(|size| size / 16 * 16)
        else {
            return Ok(false);
        };
        if frame_size <= 8 {
            return Ok(false);
        }
        let local_offset = 8i16;
        let mut planned = Vec::with_capacity(store_statements.len());
        let mut stack_loads = Vec::new();
        for (statement, parameter) in store_statements.iter().zip(&function.parameters[1..]) {
            let Statement::Store {
                target:
                    Expression::Member {
                        base,
                        offset,
                        member_type,
                        index_stride: None,
                    },
                value: Expression::Variable(source),
            } = statement
            else {
                return Ok(false);
            };
            if !matches!(base.as_ref(), Expression::Variable(name) if name == &local.name)
                || source != &parameter.name
            {
                return Ok(false);
            }
            let Ok(offset) = i16::try_from(*offset) else {
                return Ok(false);
            };
            let width = member_type.width();
            let Some(location) = self.locations.get(&parameter.name) else {
                return Ok(false);
            };
            let source = match location.class {
                ValueClass::Float => {
                    if !matches!(member_type, Type::Float | Type::Double) {
                        return Ok(false);
                    }
                    StoreSource::Float(location.register)
                }
                ValueClass::General if location.register <= 10 => {
                    StoreSource::General(location.register)
                }
                ValueClass::General => {
                    let Some(stack_index) = location.register.checked_sub(11) else {
                        return Ok(false);
                    };
                    let destination = match stack_loads.len() {
                        0 => 11,
                        1 => 0,
                        _ => return Ok(false),
                    };
                    let lane_offset = frame_size
                        .checked_add(8 + i16::from(stack_index) * 4)
                        .and_then(|offset| {
                            offset.checked_add(match parameter.parameter_type.width() {
                                8 => 3,
                                16 => 2,
                                32 => 0,
                                _ => return None,
                            })
                        });
                    let Some(lane_offset) = lane_offset else {
                        return Ok(false);
                    };
                    let load = match parameter.parameter_type {
                        Type::Char | Type::UnsignedChar => Instruction::LoadByteZero {
                            d: destination,
                            a: 1,
                            offset: lane_offset,
                        },
                        Type::Short => Instruction::LoadHalfwordAlgebraic {
                            d: destination,
                            a: 1,
                            offset: lane_offset,
                        },
                        Type::UnsignedShort => Instruction::LoadHalfwordZero {
                            d: destination,
                            a: 1,
                            offset: lane_offset,
                        },
                        Type::Int
                        | Type::UnsignedInt
                        | Type::Pointer(_)
                        | Type::StructPointer { .. } => Instruction::LoadWord {
                            d: destination,
                            a: 1,
                            offset: lane_offset,
                        },
                        _ => return Ok(false),
                    };
                    stack_loads.push(load);
                    StoreSource::General(destination)
                }
            };
            planned.push(PlannedStore {
                source,
                offset: local_offset + offset,
                width,
            });
        }
        if planned.first().is_none_or(|store| {
            !matches!(store.source, StoreSource::General(4)) || store.width != 32
        }) {
            return Ok(false);
        }

        self.non_leaf = true;
        self.frame_size = frame_size;
        self.output.instructions.extend([
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -frame_size,
            },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: frame_size + 4,
            },
        ]);
        for load in stack_loads {
            self.output.instructions.push(load);
        }
        emit_planned_store(&mut self.output.instructions, planned[0])?;
        self.output.instructions.extend([
            Instruction::Or { a: 4, s: 3, b: 3 },
            Instruction::AddImmediate {
                d: 3,
                a: 1,
                immediate: local_offset,
            },
        ]);
        for store in &planned[1..] {
            emit_planned_store(&mut self.output.instructions, *store)?;
        }
        self.record_relocation(RelocationKind::Rel24, callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: callee.clone(),
        });
        self.emit_epilogue_and_return();
        Ok(true)
    }
}

fn emit_planned_store(
    instructions: &mut Vec<Instruction>,
    store: PlannedStore,
) -> Compilation<()> {
    let instruction = match (store.source, store.width) {
        (StoreSource::General(source), 8) => Instruction::StoreByte {
            s: source,
            a: 1,
            offset: store.offset,
        },
        (StoreSource::General(source), 16) => Instruction::StoreHalfword {
            s: source,
            a: 1,
            offset: store.offset,
        },
        (StoreSource::General(source), 32) => Instruction::StoreWord {
            s: source,
            a: 1,
            offset: store.offset,
        },
        (StoreSource::Float(source), 32) => Instruction::StoreFloatSingle {
            s: source,
            a: 1,
            offset: store.offset,
        },
        (StoreSource::Float(source), 64) => Instruction::StoreFloatDouble {
            s: source,
            a: 1,
            offset: store.offset,
        },
        _ => {
            return Err(Diagnostic::error(
                "aggregate parameter forwarding has an unsupported store width",
            ))
        }
    };
    instructions.push(instruction);
    Ok(())
}
