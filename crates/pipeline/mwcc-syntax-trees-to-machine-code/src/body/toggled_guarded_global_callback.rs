//! Mutually exclusive guarded callback tables selected by a stored bit.
//!
//! Inline expansion exposes the false arm's callback wrapper while the true
//! arm remains a direct table dispatch. MWCC treats both arms as one saved
//! receiver transaction, retaining the item pointer and boolean across either
//! indirect call before writing the boolean back to its bit field.

use super::guarded_global_callback::{callback_statement, member_offset, variable};
#[allow(unused_imports)]
use super::*;

struct BitField {
    offset: i16,
    shift: u8,
    width: u8,
}

struct Shape<'a> {
    object: &'a str,
    flag: &'a str,
    object_alias_offset: i16,
    item_offset: i16,
    selector_offset: i16,
    flag_field: BitField,
    false_table: &'a str,
    true_table: &'a str,
}

fn zero(expression: &Expression) -> bool {
    constant_value(expression) == Some(0)
}

fn variable_nonzero(expression: &Expression, name: &str) -> bool {
    matches!(
        expression,
        Expression::Binary {
            operator: BinaryOperator::NotEqual,
            left,
            right,
        } if variable(left, name) && zero(right)
    )
}

fn bit_field(expression: &Expression, base: &str) -> Option<BitField> {
    let Expression::BitFieldRead {
        storage,
        shift,
        width,
        ..
    } = expression
    else {
        return None;
    };
    let Expression::Member {
        base: storage_base,
        offset,
        member_type,
        index_stride: None,
    } = storage.as_ref()
    else {
        return None;
    };
    if !variable(storage_base, base)
        || *width == 0
        || u16::from(*shift) + u16::from(*width) > u16::from(member_type.width())
    {
        return None;
    }
    Some(BitField {
        offset: i16::try_from(*offset).ok()?,
        shift: *shift,
        width: *width,
    })
}

fn same_bit_field(left: &BitField, right: &BitField) -> bool {
    left.offset == right.offset && left.shift == right.shift && left.width == right.width
}

fn inline_null_guard<'a>(
    statement: &'a Statement,
    alias: &str,
    item_offset: i16,
) -> Option<&'a str> {
    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Equal,
                left,
                right,
            },
        then_body,
        else_body,
    } = statement
    else {
        return None;
    };
    let [Statement::Goto(label)] = then_body.as_slice() else {
        return None;
    };
    (zero(right) && else_body.is_empty() && member_offset(left, alias) == Some(item_offset))
        .then_some(label)
}

fn classify(function: &Function) -> Option<Shape<'_>> {
    if function.return_type != Type::Void
        || function.return_expression.is_some()
        || !function.guards.is_empty()
    {
        return None;
    }
    let [object, flag] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(
        object.parameter_type,
        Type::Pointer(_) | Type::StructPointer { .. }
    ) || !matches!(flag.parameter_type, Type::Int | Type::UnsignedInt)
    {
        return None;
    }
    let [object_alias, item, padding, inline_alias] = function.locals.as_slice() else {
        return None;
    };
    let object_alias_offset = member_offset(object_alias.initializer.as_ref()?, &object.name)?;
    let item_offset = member_offset(item.initializer.as_ref()?, &object_alias.name)?;
    if padding.declared_type != Type::UnsignedChar
        || padding.array_length != Some(8)
        || inline_alias.initializer.is_some()
        || !matches!(
            inline_alias.declared_type,
            Type::Pointer(_) | Type::StructPointer { .. }
        )
    {
        return None;
    }

    let [outer, final_store] = function.statements.as_slice() else {
        return None;
    };
    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::LogicalAnd,
                left,
                right,
            },
        then_body,
        else_body,
    } = outer
    else {
        return None;
    };
    if !variable_nonzero(left, &item.name) || !else_body.is_empty() {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::NotEqual,
        left: tested_field,
        right: tested_flag,
    } = right.as_ref()
    else {
        return None;
    };
    let tested_field = bit_field(tested_field, &object_alias.name)?;
    if !variable(tested_flag, &flag.name) {
        return None;
    }

    let [Statement::If {
        condition:
            Expression::Unary {
                operator: UnaryOperator::LogicalNot,
                operand,
            },
        then_body: false_arm,
        else_body: true_arm,
    }] = then_body.as_slice()
    else {
        return None;
    };
    if !variable(operand, &flag.name) {
        return None;
    }

    let [Statement::Assign { name, value }, null_guard, false_callback, Statement::Label(label)] =
        false_arm.as_slice()
    else {
        return None;
    };
    if name != &inline_alias.name
        || member_offset(value, &object.name) != Some(object_alias_offset)
        || inline_null_guard(null_guard, &inline_alias.name, item_offset) != Some(label)
    {
        return None;
    }
    let (false_table, false_selector) =
        callback_statement(false_callback, &inline_alias.name, &object.name, None)?;

    let [Statement::If {
        condition: true_item_guard,
        then_body: true_body,
        else_body: true_else,
    }] = true_arm.as_slice()
    else {
        return None;
    };
    let [true_callback] = true_body.as_slice() else {
        return None;
    };
    if !variable_nonzero(true_item_guard, &item.name) || !true_else.is_empty() {
        return None;
    }
    let (true_table, true_selector) =
        callback_statement(true_callback, &object_alias.name, &object.name, None)?;

    let Statement::Store {
        target,
        value: stored_flag,
    } = final_store
    else {
        return None;
    };
    let stored_field = bit_field(target, &object_alias.name)?;
    if !same_bit_field(&tested_field, &stored_field)
        || !variable(stored_flag, &flag.name)
        || tested_field.width != 1
        || false_selector != true_selector
        || !function.locals.iter().take(2).all(|local| {
            matches!(
                local.declared_type,
                Type::Pointer(_) | Type::StructPointer { .. }
            )
        })
    {
        return None;
    }

    Some(Shape {
        object: &object.name,
        flag: &flag.name,
        object_alias_offset,
        item_offset,
        selector_offset: false_selector,
        flag_field: stored_field,
        false_table,
        true_table,
    })
}

impl Generator {
    pub(crate) fn try_toggled_guarded_global_callback(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        if self.general_register_of(shape.object)? != 3
            || self.general_register_of(shape.flag)? != 4
            || !self.globals.contains_key(shape.false_table)
            || !self.globals.contains_key(shape.true_table)
        {
            return Ok(false);
        }

        self.output.pre_scheduled = true;
        self.non_leaf = true;
        self.frame_size = 40;
        let true_arm = self.fresh_label();
        let done = self.fresh_label();

        self.output.instructions.extend([
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -40,
            },
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 36,
            },
            Instruction::StoreWord {
                s: 30,
                a: 1,
                offset: 32,
            },
            Instruction::move_register(30, 4),
            Instruction::LoadWord {
                d: 31,
                a: 3,
                offset: shape.object_alias_offset,
            },
            Instruction::LoadWord {
                d: 4,
                a: 31,
                offset: shape.item_offset,
            },
            Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 },
        ]);
        self.emit_branch_conditional_to(12, 2, done);
        self.output.instructions.extend([
            Instruction::LoadByteZero {
                d: 0,
                a: 31,
                offset: shape.flag_field.offset,
            },
            Instruction::RotateAndMask {
                a: 0,
                s: 0,
                shift: 32 - shape.flag_field.shift,
                begin: 31,
                end: 31,
            },
            Instruction::CompareWord { a: 0, b: 30 },
        ]);
        self.emit_branch_conditional_to(12, 2, done);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 30,
                immediate: 0,
            });
        self.emit_branch_conditional_to(4, 2, true_arm);

        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, done);
        self.emit_toggled_callback_arm(31, shape.selector_offset, shape.false_table, done);
        self.emit_branch_to(done);

        self.bind_label(true_arm);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, done);
        self.emit_toggled_callback_arm(31, shape.selector_offset, shape.true_table, done);

        self.bind_label(done);
        self.output.instructions.extend([
            Instruction::LoadByteZero {
                d: 0,
                a: 31,
                offset: shape.flag_field.offset,
            },
            Instruction::RotateAndMaskInsert {
                a: 0,
                s: 30,
                shift: shape.flag_field.shift,
                begin: 32 - shape.flag_field.shift - shape.flag_field.width,
                end: 31 - shape.flag_field.shift,
            },
            Instruction::StoreByte {
                s: 0,
                a: 31,
                offset: shape.flag_field.offset,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 44,
            },
            Instruction::LoadWord {
                d: 31,
                a: 1,
                offset: 36,
            },
            Instruction::LoadWord {
                d: 30,
                a: 1,
                offset: 32,
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 40,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }

    fn emit_toggled_callback_arm(
        &mut self,
        alias: u8,
        selector_offset: i16,
        table: &str,
        done: mwcc_vreg::Label,
    ) {
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: alias,
            offset: selector_offset,
        });
        self.emit_address_high(4, table);
        self.record_relocation(RelocationKind::Addr16Lo, table);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 4,
                s: 5,
                shift: 2,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 0, b: 4 });
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 12,
                immediate: 0,
            });
        self.emit_branch_conditional_to(12, 2, done);
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 12 });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegisterAndLink);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_wider_state_field() {
        let expression = Expression::BitFieldRead {
            extracted: Box::new(Expression::IntegerLiteral(0)),
            promoted_type: Type::Int,
            storage: Box::new(Expression::Member {
                base: Box::new(Expression::Variable("state".into())),
                offset: 12,
                member_type: Type::UnsignedChar,
                index_stride: None,
            }),
            shift: 4,
            width: 2,
        };
        let wider = bit_field(&expression, "state").unwrap();
        assert!(!same_bit_field(
            &wider,
            &BitField {
                offset: 12,
                shift: 4,
                width: 1,
            },
        ));
    }
}
