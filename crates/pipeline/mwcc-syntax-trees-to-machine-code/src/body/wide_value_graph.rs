//! Typed straight-line values shared by scalar and pair-register operations.
//!
//! The graph fixes memory/call ordering and assignment identities before
//! selection. A 64-bit value always has two explicit virtual-register words;
//! ordinary liveness, scheduling and frame allocation preserve both across
//! calls. Constants remain operands until use, avoiding artificial survivors.

use super::*;
use std::collections::HashMap;

fn supported(ty: Type) -> bool {
    matches!(
        ty,
        Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::LongLong | Type::UnsignedLongLong
    )
}
fn wide(ty: Type) -> bool {
    matches!(ty, Type::LongLong | Type::UnsignedLongLong)
}

#[derive(Clone, Copy, Debug)]
struct Value {
    ty: Type,
    source: Source,
}
#[derive(Clone, Copy, Debug)]
enum Source {
    Constant(u64),
    Register(usize),
}
#[derive(Debug)]
enum Operation {
    Parameter {
        result: Value,
        high: u32,
    },
    Address {
        result: Value,
        symbol: String,
    },
    Load {
        result: Value,
        pointer: Value,
    },
    Store {
        pointer: Value,
        value: Value,
    },
    Convert {
        result: Value,
        value: Value,
    },
    Binary {
        result: Value,
        operator: BinaryOperator,
        left: Value,
        right: Value,
    },
    Call {
        name: String,
        arguments: Vec<(u32, Value)>,
        result: Option<Value>,
    },
}

struct Graph<'a> {
    operations: Vec<Operation>,
    values: usize,
    bindings: HashMap<String, Value>,
    types: HashMap<String, Type>,
    globals: &'a HashMap<String, Type>,
    returns: &'a HashMap<String, Type>,
    parameters: &'a HashMap<String, Vec<Type>>,
    returned: Option<Value>,
}

/// The word/pair-only EABI subset. A pair starts at an odd GPR. Reject overflow
/// before emitting anything: stack arguments need their own pair slot layout.
fn abi_registers(types: impl IntoIterator<Item = Type>) -> Option<Vec<u32>> {
    let mut next = u32::from(Eabi::FIRST_GENERAL_ARGUMENT);
    let mut registers = Vec::new();
    for ty in types {
        if !supported(ty) {
            return None;
        }
        if wide(ty) && next % 2 == 0 {
            next += 1;
        }
        let words = if wide(ty) { 2 } else { 1 };
        if next + words - 1 > u32::from(Eabi::LAST_GENERAL_ARGUMENT) {
            return None;
        }
        registers.push(next);
        next += words;
    }
    Some(registers)
}

impl<'a> Graph<'a> {
    fn fresh(&mut self, ty: Type) -> Value {
        let value = Value {
            ty,
            source: Source::Register(self.values),
        };
        self.values += 1;
        value
    }

    fn convert(&mut self, value: Value, ty: Type) -> Option<Value> {
        if !supported(ty) {
            return None;
        }
        if let Source::Constant(bits) = value.source {
            let bits = if !wide(ty) {
                bits & 0xffff_ffff
            } else if !wide(value.ty) && value.ty.is_signed() {
                (bits as i32 as i64) as u64
            } else {
                bits
            };
            return Some(Value {
                ty,
                source: Source::Constant(bits),
            });
        }
        if wide(ty) == wide(value.ty) {
            return Some(Value { ty, ..value });
        }
        let result = self.fresh(ty);
        self.operations.push(Operation::Convert { result, value });
        Some(result)
    }

    fn address(&mut self, name: &str) -> Option<Value> {
        let ty = *self.globals.get(name)?;
        if !supported(ty) || self.types.contains_key(name) {
            return None;
        }
        // The pointee identity is supplied by the load/store, so this internal
        // address is an unsigned word independent of the source object width.
        let result = self.fresh(Type::Pointer(Pointee::UnsignedInt));
        self.operations.push(Operation::Address {
            result,
            symbol: name.into(),
        });
        Some(result)
    }

    fn expression(&mut self, expression: &Expression) -> Option<Value> {
        match expression {
            Expression::IntegerLiteral(bits) => Some(Value {
                ty: if i32::try_from(*bits).is_ok() {
                    Type::Int
                } else if u32::try_from(*bits).is_ok() {
                    Type::UnsignedInt
                } else {
                    Type::LongLong
                },
                source: Source::Constant(*bits as u64),
            }),
            Expression::Variable(name) => {
                if let Some(value) = self.bindings.get(name) {
                    return Some(*value);
                }
                let ty = *self.globals.get(name)?;
                let pointer = self.address(name)?;
                let result = self.fresh(ty);
                self.operations.push(Operation::Load { result, pointer });
                Some(result)
            }
            Expression::Cast {
                target_type,
                operand,
            } => {
                let value = self.expression(operand)?;
                self.convert(value, *target_type)
            }
            Expression::AddressOf { operand } => {
                let Expression::Variable(name) = operand.as_ref() else {
                    return None;
                };
                let address = self.address(name)?;
                let ty = match self.globals.get(name)? {
                    Type::LongLong => Type::Pointer(Pointee::LongLong),
                    Type::UnsignedLongLong => Type::Pointer(Pointee::UnsignedLongLong),
                    Type::Int => Type::Pointer(Pointee::Int),
                    Type::UnsignedInt => Type::Pointer(Pointee::UnsignedInt),
                    _ => return None,
                };
                Some(Value { ty, ..address })
            }
            Expression::Dereference { pointer } => {
                let pointer = self.expression(pointer)?;
                let Type::Pointer(pointee) = pointer.ty else {
                    return None;
                };
                let ty = pointee.element();
                if !supported(ty) {
                    return None;
                }
                let result = self.fresh(ty);
                self.operations.push(Operation::Load { result, pointer });
                Some(result)
            }
            Expression::Call { name, arguments } => self.call(name, arguments, true)?,
            Expression::Binary {
                operator,
                left,
                right,
            } if matches!(
                operator,
                BinaryOperator::Add
                    | BinaryOperator::Subtract
                    | BinaryOperator::BitAnd
                    | BinaryOperator::BitOr
                    | BinaryOperator::BitXor
            ) =>
            {
                // Call-bearing RHS first matches the time-adjust transaction:
                // obtain the current clock, then read the adjustment memory.
                // Separate statements and explicit comma expressions never
                // enter this rule, so their sequencing cannot be crossed.
                let (left, right) = if expression_has_side_effect(right) {
                    let right = self.expression(right)?;
                    (self.expression(left)?, right)
                } else {
                    let left = self.expression(left)?;
                    (left, self.expression(right)?)
                };
                if matches!(left.ty, Type::Pointer(_)) || matches!(right.ty, Type::Pointer(_)) {
                    return None;
                }
                let ty = if left.ty == Type::UnsignedLongLong || right.ty == Type::UnsignedLongLong
                {
                    Type::UnsignedLongLong
                } else if wide(left.ty) || wide(right.ty) {
                    Type::LongLong
                } else if left.ty == Type::UnsignedInt || right.ty == Type::UnsignedInt {
                    Type::UnsignedInt
                } else {
                    Type::Int
                };
                let left = self.convert(left, ty)?;
                let right = self.convert(right, ty)?;
                if let (Source::Constant(left), Source::Constant(right)) =
                    (left.source, right.source)
                {
                    let bits = match operator {
                        BinaryOperator::Add => left.wrapping_add(right),
                        BinaryOperator::Subtract => left.wrapping_sub(right),
                        BinaryOperator::BitAnd => left & right,
                        BinaryOperator::BitOr => left | right,
                        BinaryOperator::BitXor => left ^ right,
                        _ => unreachable!(),
                    };
                    return Some(Value {
                        ty,
                        source: Source::Constant(if wide(ty) { bits } else { bits & 0xffff_ffff }),
                    });
                }
                let result = self.fresh(ty);
                self.operations.push(Operation::Binary {
                    result,
                    operator: *operator,
                    left,
                    right,
                });
                Some(result)
            }
            _ => None,
        }
    }

    fn call(&mut self, name: &str, arguments: &[Expression], used: bool) -> Option<Option<Value>> {
        if self.types.contains_key(name) || self.globals.contains_key(name) {
            return None;
        }
        let formals = self.parameters.get(name)?.clone();
        if formals.len() != arguments.len() {
            return None;
        }
        let registers = abi_registers(formals.iter().copied())?;
        let mut inputs = Vec::new();
        // Evaluate all arguments before touching ABI destinations. Physical
        // parallel-copy constraints are then visible to the normal allocator.
        for ((argument, ty), high) in arguments.iter().zip(formals).zip(registers) {
            let value = self.expression(argument)?;
            inputs.push((high, self.convert(value, ty)?));
        }
        let ty = *self.returns.get(name)?;
        if !(supported(ty) || ty == Type::Void) || (used && ty == Type::Void) {
            return None;
        }
        let result = used.then(|| self.fresh(ty));
        self.operations.push(Operation::Call {
            name: name.into(),
            arguments: inputs,
            result,
        });
        Some(result)
    }

    fn assign(&mut self, name: &str, expression: &Expression) -> Option<()> {
        let ty = *self.types.get(name)?;
        let value = self.expression(expression)?;
        let value = self.convert(value, ty)?;
        self.bindings.insert(name.into(), value);
        Some(())
    }

    fn recognize(
        function: &Function,
        globals: &'a HashMap<String, Type>,
        returns: &'a HashMap<String, Type>,
        parameters: &'a HashMap<String, Vec<Type>>,
    ) -> Option<Self> {
        if !(supported(function.return_type) || function.return_type == Type::Void)
            || !function.guards.is_empty()
            || function.locals.iter().any(|l| {
                !supported(l.declared_type)
                    || l.is_volatile
                    || l.is_static
                    || l.array_length.is_some()
            })
        {
            return None;
        }
        let incoming = abi_registers(function.parameters.iter().map(|p| p.parameter_type))?;
        let mut graph = Self {
            operations: Vec::new(),
            values: 0,
            bindings: HashMap::new(),
            types: function
                .locals
                .iter()
                .map(|l| (l.name.clone(), l.declared_type))
                .chain(
                    function
                        .parameters
                        .iter()
                        .map(|p| (p.name.clone(), p.parameter_type)),
                )
                .collect(),
            globals,
            returns,
            parameters,
            returned: None,
        };
        for (parameter, high) in function.parameters.iter().zip(incoming) {
            let result = graph.fresh(parameter.parameter_type);
            graph.operations.push(Operation::Parameter { result, high });
            graph.bindings.insert(parameter.name.clone(), result);
        }
        for local in &function.locals {
            if let Some(value) = &local.initializer {
                graph.assign(&local.name, value)?;
            }
        }
        let mut returned = function.return_expression.as_ref();
        for (index, statement) in function.statements.iter().enumerate() {
            match statement {
                Statement::Assign { name, value } => graph.assign(name, value)?,
                Statement::Expression(Expression::Assign { target, value }) => {
                    let Expression::Variable(name) = target.as_ref() else {
                        return None;
                    };
                    graph.assign(name, value)?;
                }
                Statement::Expression(Expression::Call { name, arguments }) => {
                    graph.call(name, arguments, false)?;
                }
                Statement::Store { target, value } => {
                    let value = graph.expression(value)?;
                    let (pointer, ty) = match target {
                        Expression::Variable(name) => (graph.address(name)?, *globals.get(name)?),
                        Expression::Dereference { pointer } => {
                            let pointer = graph.expression(pointer)?;
                            let Type::Pointer(pointee) = pointer.ty else {
                                return None;
                            };
                            (pointer, pointee.element())
                        }
                        _ => return None,
                    };
                    let value = graph.convert(value, ty)?;
                    graph.operations.push(Operation::Store { pointer, value });
                }
                Statement::Return(value)
                    if index + 1 == function.statements.len() && returned.is_none() =>
                {
                    returned = value.as_ref()
                }
                _ => return None,
            }
        }
        if function.return_type == Type::Void {
            if returned.is_some() {
                return None;
            }
        } else {
            let value = graph.expression(returned?)?;
            graph.returned = Some(graph.convert(value, function.return_type)?);
        }
        Some(graph)
    }
}

/// Scalar words use `low`; pairs use `high:low` in big-endian ABI order.
#[derive(Clone, Copy)]
struct Registers {
    high: Option<u32>,
    low: u32,
}

impl Generator {
    pub(crate) fn try_wide_value_graph(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !self.output.instructions.is_empty() {
            return Ok(false);
        }
        let Some(graph) = Graph::recognize(
            function,
            &self.globals,
            &self.call_return_types,
            &self.call_parameter_types,
        ) else {
            return Ok(false);
        };
        let operations = graph.operations;
        let returned = graph.returned;
        let mut registers = vec![None; graph.values];
        // Explicit pairs and memory sequencing are selected here. The allocator
        // still owns survivors and their frame; do not reschedule split loads
        // across calls or separate a carry producer from its consumer.
        self.output.pre_scheduled = true;
        if operations
            .iter()
            .any(|op| matches!(op, Operation::Call { .. }))
        {
            // Allocation grows this canonical frame and then applies the
            // generation's linkage-first/predecrement convention.
            self.non_leaf = true;
            self.frame_size = 16;
            self.output.instructions.extend([
                Instruction::StoreWordWithUpdate {
                    s: 1,
                    a: 1,
                    offset: -16,
                },
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: 20,
                },
            ]);
        }
        for operation in operations {
            match operation {
                Operation::Parameter { result, high } => {
                    let destination = self.wide_graph_destination(result, &mut registers);
                    if let Some(d) = destination.high {
                        self.output
                            .instructions
                            .push(Instruction::move_register(destination.low, high + 1));
                        self.output
                            .instructions
                            .push(Instruction::move_register(d, high));
                    } else {
                        self.output
                            .instructions
                            .push(Instruction::move_register(destination.low, high));
                    }
                }
                Operation::Address { result, symbol } => {
                    let destination = self.wide_graph_destination(result, &mut registers).low;
                    self.emit_address_high(destination, &symbol);
                    self.record_relocation(RelocationKind::Addr16Lo, &symbol);
                    self.output.instructions.push(Instruction::AddImmediate {
                        d: destination,
                        a: destination,
                        immediate: 0,
                    });
                }
                Operation::Load { result, pointer } => {
                    let pointer = self.wide_graph_operand(pointer, &registers).low;
                    let destination = self.wide_graph_destination(result, &mut registers);
                    if let Some(high) = destination.high {
                        self.output.instructions.push(Instruction::LoadWord {
                            d: high,
                            a: pointer,
                            offset: 0,
                        });
                        self.output.instructions.push(Instruction::LoadWord {
                            d: destination.low,
                            a: pointer,
                            offset: 4,
                        });
                    } else {
                        self.output.instructions.push(Instruction::LoadWord {
                            d: destination.low,
                            a: pointer,
                            offset: 0,
                        });
                    }
                }
                Operation::Store { pointer, value } => {
                    let pointer = self.wide_graph_operand(pointer, &registers).low;
                    let value = self.wide_graph_operand(value, &registers);
                    self.output.instructions.push(Instruction::StoreWord {
                        s: value.low,
                        a: pointer,
                        offset: if value.high.is_some() { 4 } else { 0 },
                    });
                    if let Some(high) = value.high {
                        self.output.instructions.push(Instruction::StoreWord {
                            s: high,
                            a: pointer,
                            offset: 0,
                        });
                    }
                }
                Operation::Convert { result, value } => {
                    let source = self.wide_graph_operand(value, &registers);
                    let destination = self.wide_graph_destination(result, &mut registers);
                    self.output
                        .instructions
                        .push(Instruction::move_register(destination.low, source.low));
                    if let Some(high) = destination.high {
                        if value.ty.is_signed() {
                            self.output.instructions.push(
                                Instruction::ShiftRightAlgebraicImmediate {
                                    a: high,
                                    s: source.low,
                                    shift: 31,
                                },
                            );
                        } else {
                            self.load_integer_constant(high, 0);
                        }
                    }
                }
                Operation::Binary {
                    result,
                    operator,
                    left,
                    right,
                } => {
                    let left = self.wide_graph_operand(left, &registers);
                    let right = self.wide_graph_operand(right, &registers);
                    let destination = self.wide_graph_destination(result, &mut registers);
                    let instruction = |d, a, b, high| match operator {
                        BinaryOperator::Add if high => Instruction::AddExtended { d, a, b },
                        BinaryOperator::Add if destination.high.is_some() => {
                            Instruction::AddCarrying { d, a, b }
                        }
                        BinaryOperator::Add => Instruction::Add { d, a, b },
                        BinaryOperator::Subtract if high => {
                            Instruction::SubtractFromExtended { d, a: b, b: a }
                        }
                        BinaryOperator::Subtract if destination.high.is_some() => {
                            Instruction::SubtractFromCarrying { d, a: b, b: a }
                        }
                        BinaryOperator::Subtract => Instruction::SubtractFrom { d, a: b, b: a },
                        BinaryOperator::BitAnd => Instruction::And { a: d, s: a, b },
                        BinaryOperator::BitOr => Instruction::Or { a: d, s: a, b },
                        BinaryOperator::BitXor => Instruction::Xor { a: d, s: a, b },
                        _ => unreachable!(),
                    };
                    self.output.instructions.push(instruction(
                        destination.low,
                        left.low,
                        right.low,
                        false,
                    ));
                    if let Some(high) = destination.high {
                        self.output.instructions.push(instruction(
                            high,
                            left.high.unwrap(),
                            right.high.unwrap(),
                            true,
                        ));
                    }
                }
                Operation::Call {
                    name,
                    arguments,
                    result,
                } => {
                    // Materialize every operand before starting the ABI copy
                    // group; constants must not clobber already filled inputs.
                    let arguments: Vec<_> = arguments
                        .into_iter()
                        .map(|(high, value)| (high, self.wide_graph_operand(value, &registers)))
                        .collect();
                    for (high, value) in arguments.into_iter().rev() {
                        if let Some(source) = value.high {
                            self.output
                                .instructions
                                .push(Instruction::move_register(high + 1, value.low));
                            self.output
                                .instructions
                                .push(Instruction::move_register(high, source));
                        } else {
                            self.output
                                .instructions
                                .push(Instruction::move_register(high, value.low));
                        }
                    }
                    self.record_relocation(RelocationKind::Rel24, &name);
                    self.output
                        .instructions
                        .push(Instruction::BranchAndLink { target: name });
                    if let Some(result) = result {
                        let destination = self.wide_graph_destination(result, &mut registers);
                        self.output.instructions.push(Instruction::move_register(
                            destination.low,
                            if destination.high.is_some() { 4 } else { 3 },
                        ));
                        if let Some(high) = destination.high {
                            self.output
                                .instructions
                                .push(Instruction::move_register(high, 3));
                        }
                    }
                }
            }
        }
        if let Some(value) = returned {
            if let Source::Constant(bits) = value.source {
                // The return ABI is the destination of a terminal constant;
                // do not create a temporary pair and then copy it back.
                self.load_integer_constant(if wide(value.ty) { 4 } else { 3 }, bits as i64);
                if wide(value.ty) {
                    self.load_integer_constant(3, (bits >> 32) as i64);
                }
            } else {
                let value = self.wide_graph_operand(value, &registers);
                self.output.instructions.push(Instruction::move_register(
                    if value.high.is_some() { 4 } else { 3 },
                    value.low,
                ));
                if let Some(high) = value.high {
                    self.output
                        .instructions
                        .push(Instruction::move_register(3, high));
                }
            }
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    fn wide_graph_destination(
        &mut self,
        value: Value,
        registers: &mut [Option<Registers>],
    ) -> Registers {
        let Source::Register(id) = value.source else {
            unreachable!()
        };
        let destination = Registers {
            low: self.fresh_virtual_general(),
            high: wide(value.ty).then(|| self.fresh_virtual_general()),
        };
        registers[id] = Some(destination);
        destination
    }

    fn wide_graph_operand(&mut self, value: Value, registers: &[Option<Registers>]) -> Registers {
        match value.source {
            Source::Register(id) => registers[id].expect("graph values are defined before use"),
            Source::Constant(bits) => {
                let low = self.fresh_virtual_general();
                self.load_integer_constant(low, bits as i64);
                let high = wide(value.ty).then(|| {
                    let high = self.fresh_virtual_general();
                    self.load_integer_constant(high, (bits >> 32) as i64);
                    high
                });
                Registers { high, low }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_pair_abi_pads_and_rejects_overflow() {
        assert_eq!(
            abi_registers([Type::Int, Type::LongLong, Type::Int]),
            Some(vec![3, 5, 7])
        );
        assert_eq!(abi_registers([Type::LongLong; 4]), Some(vec![3, 5, 7, 9]));
        assert!(abi_registers([Type::LongLong; 5]).is_none());
        assert!(abi_registers([Type::Int, Type::Double]).is_none());
    }

    #[test]
    fn word_promotion_preserves_source_signedness() {
        let globals = HashMap::new();
        let returns = HashMap::new();
        let parameters = HashMap::new();
        let mut graph = Graph {
            operations: Vec::new(),
            values: 0,
            bindings: HashMap::new(),
            types: HashMap::new(),
            globals: &globals,
            returns: &returns,
            parameters: &parameters,
            returned: None,
        };
        for (ty, expected) in [(Type::Int, u64::MAX), (Type::UnsignedInt, 0xffff_ffff)] {
            let input = Value {
                ty,
                source: Source::Constant(0xffff_ffff),
            };
            let value = graph.convert(input, Type::UnsignedLongLong).unwrap();
            assert!(matches!(value.source, Source::Constant(bits) if bits == expected));
        }
        let wide_value = Value {
            ty: Type::UnsignedLongLong,
            source: Source::Constant(0x123456789abcdef0),
        };
        let low = graph.convert(wide_value, Type::UnsignedInt).unwrap();
        assert!(matches!(low.source, Source::Constant(0x9abcdef0)));
        assert!(graph.convert(wide_value, Type::Double).is_none());
        let sum = Expression::Binary {
            operator: BinaryOperator::Add,
            left: Box::new(Expression::Cast {
                target_type: Type::UnsignedLongLong,
                operand: Box::new(Expression::IntegerLiteral(0xffff_ffff)),
            }),
            right: Box::new(Expression::IntegerLiteral(1)),
        };
        let folded = graph.expression(&sum).unwrap();
        assert!(matches!(folded.source, Source::Constant(0x100000000)));
        assert!(graph.operations.is_empty());
    }
}
