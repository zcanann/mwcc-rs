//! Single-use pointer parameter images around GC/1.1p1 O0 indirect calls.
//!
//! Both incoming pointers occupy SP+8. Argument and callee loads must observe
//! the last declaration-order store, even when it names the wrong object.

use super::structured_expression_visit::{rewrite_expression, visit_expression};
use super::*;

fn word(ty: Type) -> bool {
    matches!(ty, Type::Int | Type::UnsignedInt)
}

fn pointer(ty: Type) -> bool {
    matches!(ty, Type::Pointer(_) | Type::StructPointer { .. })
}

fn uncast(value: &Expression) -> &Expression {
    match value {
        Expression::Cast {
            target_type,
            operand,
        } if pointer(*target_type) => uncast(operand),
        _ => value,
    }
}

fn member(value: &Expression) -> Option<(&str, u32, Type)> {
    let Expression::Member {
        base,
        offset,
        member_type,
        index_stride: None,
    } = value
    else {
        return None;
    };
    let Expression::Variable(name) = uncast(base) else {
        return None;
    };
    Some((name, *offset, *member_type))
}

fn result_expression(value: &Expression, result: &str) -> bool {
    match value {
        // The result home is a pointer. Only its scalar members belong to this
        // return plan; pointer arithmetic needs the original pointee stride.
        Expression::Variable(_) => false,
        Expression::IntegerLiteral(_) => true,
        Expression::Member { .. } => {
            member(value).is_some_and(|(name, _, ty)| name == result && word(ty))
        }
        Expression::Cast {
            target_type,
            operand,
        } if word(*target_type) => result_expression(operand, result),
        Expression::Binary {
            operator:
                BinaryOperator::Add
                | BinaryOperator::Subtract
                | BinaryOperator::BitAnd
                | BinaryOperator::BitOr
                | BinaryOperator::BitXor,
            left,
            right,
        } => result_expression(left, result) && result_expression(right, result),
        _ => false,
    }
}

impl Generator {
    pub(crate) fn try_shared_spill_indirect_call(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let [first, second] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if !self.behavior.unoptimized_shared_parameter_spills
            || !pointer(first.parameter_type)
            || !pointer(second.parameter_type)
            || !word(function.return_type)
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || !function.inline_asm_blocks.is_empty()
            || function.asm_body.is_some()
        {
            return Ok(false);
        }
        let Some(tail) = &function.return_expression else {
            return Ok(false);
        };
        let (result_local, call_value) = match function.locals.as_slice() {
            [] if function.statements.is_empty() => (None, tail),
            [local]
                if pointer(local.declared_type)
                    && !local.is_static
                    && !local.is_volatile
                    && local.array_length.is_none() =>
            {
                let value = match (local.initializer.as_ref(), function.statements.as_slice()) {
                    (Some(value), []) => value,
                    (None, [Statement::Assign { name, value }]) if name == &local.name => value,
                    _ => return Ok(false),
                };
                if !matches!(uncast(value), Expression::CallThrough { .. }) {
                    return Ok(false);
                }
                (Some(local.name.as_str()), value)
            }
            _ => return Ok(false),
        };
        let mut calls = Vec::new();
        visit_expression(call_value, &mut |value| {
            if let Expression::CallThrough { target, arguments } = value {
                calls.push((target.clone(), arguments.clone()));
            }
        });
        let [(target, arguments)] = calls.as_slice() else {
            return Ok(false);
        };
        let [argument] = arguments.as_slice() else {
            return Ok(false);
        };
        let (
            Some((callee_base, callee_offset, callee_type)),
            Some((argument_base, argument_offset, argument_type)),
        ) = (member(target), member(argument))
        else {
            return Ok(false);
        };
        if !pointer(callee_type)
            || !word(argument_type)
            || callee_base == argument_base
            || ![&first.name, &second.name]
                .into_iter()
                .any(|name| name == callee_base)
            || ![&first.name, &second.name]
                .into_iter()
                .any(|name| name == argument_base)
        {
            return Ok(false);
        }
        let result_name = (0..)
            .map(|n| format!("__mwcc_indirect_spill_result_{n}"))
            .find(|name| {
                !self.locations.contains_key(name)
                    && function.parameters.iter().all(|p| p.name != *name)
                    && function.locals.iter().all(|p| p.name != *name)
            })
            .unwrap();
        let result = result_local.unwrap_or(&result_name);
        let tail = if result_local.is_none() {
            rewrite_expression(tail, &mut |value| {
                matches!(value, Expression::CallThrough { .. })
                    .then(|| Expression::Variable(result.into()))
            })
        } else {
            tail.clone()
        };
        if !result_expression(&tail, result) || !expression_reads_name(&tail, result) {
            return Ok(false);
        }
        let (Ok(callee_offset), Ok(argument_offset)) =
            (i16::try_from(callee_offset), i16::try_from(argument_offset))
        else {
            return Ok(false);
        };

        self.emit_linkage_first_nonleaf_prologue(if result_local.is_some() { &[31] } else { &[] });
        for register in [3, 4] {
            self.output.instructions.push(Instruction::StoreWord {
                s: register,
                a: 1,
                offset: 8,
            });
        }
        // This patch marshals the argument before loading the indirect target.
        // Reload each source image separately; folding them into an incoming
        // pointer would hide both the spill overlap and the reference schedule.
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 3,
                a: 1,
                offset: 8,
            },
            Instruction::LoadWord {
                d: 3,
                a: 3,
                offset: argument_offset,
            },
            Instruction::LoadWord {
                d: 4,
                a: 1,
                offset: 8,
            },
            Instruction::LoadWord {
                d: 12,
                a: 4,
                offset: callee_offset,
            },
        ]);
        self.emit_indirect_branch_and_link(12);
        let home = if result_local.is_some() { 31 } else { 3 };
        if home != 3 {
            self.output
                .instructions
                .push(Instruction::move_register(home, 3));
        }
        self.locations.insert(
            result.into(),
            Location {
                class: ValueClass::General,
                register: home,
                signed: false,
                width: 32,
                pointee: None,
                stride: None,
            },
        );
        self.evaluate_general(&tail, 3)?;
        if result_local.is_none() {
            self.locations.remove(result);
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(base: &str, ty: Type) -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable(base.into())),
            offset: 12,
            member_type: ty,
            index_stride: None,
        }
    }

    #[test]
    fn the_postcall_expression_can_read_only_the_result_home() {
        let tail = Expression::Binary {
            operator: BinaryOperator::Add,
            left: Box::new(field("result", Type::UnsignedInt)),
            right: Box::new(Expression::IntegerLiteral(5)),
        };
        assert!(result_expression(&tail, "result"));
        // A parameter read after the callback needs a retained parameter home;
        // it cannot use this single-use source-spill plan.
        assert!(!result_expression(&tail, "parameter"));
        assert!(!result_expression(
            &Expression::Variable("result".into()),
            "result"
        ));
        assert!(!result_expression(
            &field("parameter", Type::UnsignedInt),
            "result"
        ));
    }

    #[test]
    fn floating_fields_and_additional_calls_keep_their_ordinary_owners() {
        assert!(!result_expression(&field("result", Type::Float), "result"));
        let extra_call = Expression::Call {
            name: "observe".into(),
            arguments: vec![Expression::Variable("result".into())],
        };
        assert!(!result_expression(&extra_call, "result"));
    }
}
