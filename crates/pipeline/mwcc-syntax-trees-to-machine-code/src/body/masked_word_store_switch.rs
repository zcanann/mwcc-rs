//! Masked register-map reads lowered through a sparse jump table.
//!
//! Memory-mapped device readers commonly normalize an address to a small
//! register window, switch on the result, write one word through an output
//! pointer, and return a shared boolean status. Legacy MWCC treats the mask,
//! table dispatch, arm stores, and shared exits as one scheduling region.

#[allow(unused_imports)]
use super::*;
use mwcc_machine_code::{JumpTable, RelocationTarget};
use mwcc_syntax_trees::ArmBody;

enum StoredWord {
    Member(i16),
    Constant(i16),
}

struct MaskedWordStoreSwitch {
    clear: u8,
    bound: u16,
    arms: Vec<(u16, StoredWord)>,
}

fn constant(expression: &Expression) -> Option<i64> {
    crate::analysis::constant_value(expression)
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn output_word_store(statement: &Statement, object: &str, output: &str) -> Option<StoredWord> {
    let Statement::Store { target, value } = statement else {
        return None;
    };
    if !matches!(
        target,
        Expression::Dereference { pointer } if variable(pointer, output)
    ) {
        return None;
    }
    if let Some(value) = constant(value).and_then(|value| i16::try_from(value).ok()) {
        return Some(StoredWord::Constant(value));
    }
    let Expression::Member {
        base,
        offset,
        member_type,
        index_stride: None,
    } = value
    else {
        return None;
    };
    if !variable(base, object) || !matches!(member_type, Type::Int | Type::UnsignedInt) {
        return None;
    }
    Some(StoredWord::Member(i16::try_from(*offset).ok()?))
}

fn classify(function: &Function) -> Option<MaskedWordStoreSwitch> {
    if function.return_type != Type::Int
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || constant(function.return_expression.as_ref()?) != Some(1)
    {
        return None;
    }
    let [object, selector, output] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(
        object.parameter_type,
        Type::StructPointer { .. } | Type::Pointer(_)
    ) || !matches!(selector.parameter_type, Type::Int | Type::UnsignedInt)
        || !matches!(
            output.parameter_type,
            Type::Pointer(Pointee::Int | Pointee::UnsignedInt)
        )
    {
        return None;
    }
    let [Statement::Assign {
        name,
        value:
            Expression::Binary {
                operator: BinaryOperator::BitAnd,
                left,
                right,
            },
    }, Statement::Switch {
        scrutinee,
        arms,
        default: Some(default),
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if name != &selector.name
        || !variable(left, &selector.name)
        || !variable(scrutinee, &selector.name)
        || constant(default.return_expression()?) != Some(0)
        || arms.len() < 4
    {
        return None;
    }
    let mask = u32::try_from(constant(right)?).ok()?;
    if mask == 0 || mask == u32::MAX || mask & mask.wrapping_add(1) != 0 {
        return None;
    }

    let mut lowered = Vec::with_capacity(arms.len());
    for arm in arms {
        let ArmBody::Statements(statements) = &arm.body else {
            return None;
        };
        let [statement] = statements.as_slice() else {
            return None;
        };
        let value = u16::try_from(arm.value).ok()?;
        if arm.falls_through || u32::from(value) > mask {
            return None;
        }
        lowered.push((
            value,
            output_word_store(statement, &object.name, &output.name)?,
        ));
    }
    lowered.sort_by_key(|(value, _)| *value);
    if lowered.first()?.0 != 0
        || lowered.windows(2).any(|pair| pair[0].0 == pair[1].0)
        || lowered.last()?.0 < 6
    {
        return None;
    }

    Some(MaskedWordStoreSwitch {
        clear: mask.leading_zeros() as u8,
        bound: lowered.last()?.0,
        arms: lowered,
    })
}

impl Generator {
    pub(crate) fn try_masked_word_store_switch(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        if self.lookup_general(&function.parameters[0].name) != Some(3)
            || self.lookup_general(&function.parameters[1].name) != Some(4)
            || self.lookup_general(&function.parameters[2].name) != Some(5)
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        self.emit_masked_word_store_switch(&shape);
        Ok(true)
    }

    fn emit_masked_word_store_switch(&mut self, shape: &MaskedWordStoreSwitch) {
        const OBJECT: u8 = 3;
        const SELECTOR: u8 = 4;
        const OUTPUT: u8 = 5;
        const SCRATCH: u8 = 0;
        const TABLE: u8 = 4;

        self.output.pre_scheduled = true;
        self.output.instructions.extend([
            Instruction::ClearLeftImmediate {
                a: SCRATCH,
                s: SELECTOR,
                clear: shape.clear,
            },
            Instruction::CompareLogicalWordImmediate {
                a: SCRATCH,
                immediate: shape.bound,
            },
        ]);
        let out_of_range = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 1,
                target: 0,
            });
        self.record_target(RelocationKind::Addr16Ha, RelocationTarget::JumpTable);
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: TABLE,
                a: 0,
                immediate: 0,
            });
        self.record_target(RelocationKind::Addr16Lo, RelocationTarget::JumpTable);
        self.output.instructions.push(Instruction::AddImmediate {
            d: TABLE,
            a: TABLE,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: SCRATCH,
                s: SCRATCH,
                shift: 2,
            });
        self.output.instructions.push(Instruction::LoadWordIndexed {
            d: SCRATCH,
            a: TABLE,
            b: SCRATCH,
        });
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: SCRATCH });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegister);

        let mut body_offsets = std::collections::HashMap::new();
        let mut success_branches = Vec::with_capacity(shape.arms.len());
        for (value, stored) in &shape.arms {
            body_offsets.insert(*value, self.output.instructions.len() as u32 * 4);
            match stored {
                StoredWord::Member(offset) => {
                    self.output.instructions.push(Instruction::LoadWord {
                        d: SCRATCH,
                        a: OBJECT,
                        offset: *offset,
                    });
                }
                StoredWord::Constant(value) => {
                    self.output
                        .instructions
                        .push(Instruction::load_immediate(SCRATCH, *value));
                }
            }
            self.output.instructions.push(Instruction::StoreWord {
                s: SCRATCH,
                a: OUTPUT,
                offset: 0,
            });
            success_branches.push(self.output.instructions.len());
            self.output
                .instructions
                .push(Instruction::Branch { target: 0 });
        }

        let default_offset = self.output.instructions.len() as u32 * 4;
        let default_index = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[out_of_range]
        {
            *target = default_index;
        }
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);

        let success = self.output.instructions.len();
        for branch in success_branches {
            if let Instruction::Branch { target } = &mut self.output.instructions[branch] {
                *target = success;
            }
        }
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 1));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);

        let entries = (0..=shape.bound)
            .map(|value| *body_offsets.get(&value).unwrap_or(&default_offset))
            .collect();
        self.output.jump_tables.push(JumpTable {
            entries,
            anonymous_offset: shape.arms.len() as u32 + 2,
        });
    }
}
