//! Conditional installation of one of two global function-pointer tables.
//!
//! Each arm calls a probe whose result remains in r3, then fills the same
//! global record with a different ordered function-address set. MWCC keeps the
//! record base in r5 for the arm and streams each address through r0.

#[allow(unused_imports)]
use super::*;

struct FunctionTableArm<'a> {
    probe: &'a str,
    entries: Vec<(i16, &'a str)>,
}

struct FunctionTableSelection<'a> {
    selector: i16,
    global: &'a str,
    then_arm: FunctionTableArm<'a>,
    else_arm: FunctionTableArm<'a>,
}

impl Generator {
    pub(crate) fn try_global_function_table_selection(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return Ok(false);
        }
        let Some(plan) = recognize(function) else {
            return Ok(false);
        };
        if (!self.addressable_globals.contains_key(plan.global)
            && !matches!(self.globals.get(plan.global), Some(Type::Struct { .. })))
            || !plan
                .then_arm
                .entries
                .iter()
                .chain(&plan.else_arm.entries)
                .all(|(_, function)| self.is_direct_function_symbol(function))
        {
            return Ok(false);
        }

        self.non_leaf = true;
        self.frame_size = 8;
        self.output.pre_scheduled = true;
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -8,
            });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: Eabi::FIRST_GENERAL_ARGUMENT,
                immediate: plan.selector,
            });
        let false_branch = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 0,
            });

        self.emit_function_table_arm(plan.global, &plan.then_arm);
        let join_branch = self.output.instructions.len();
        self.output.instructions.push(Instruction::Branch { target: 0 });
        let else_start = self.output.instructions.len();
        let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[false_branch]
        else {
            unreachable!("the table selector branch remains conditional")
        };
        *target = else_start;

        self.emit_function_table_arm(plan.global, &plan.else_arm);
        let join = self.output.instructions.len();
        let Instruction::Branch { target } = &mut self.output.instructions[join_branch] else {
            unreachable!("the table arm join remains unconditional")
        };
        *target = join;

        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 8,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    fn emit_function_table_arm(&mut self, global: &str, arm: &FunctionTableArm<'_>) {
        self.record_relocation(RelocationKind::Rel24, arm.probe);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: arm.probe.to_owned(),
        });

        let (first_offset, first_function) = arm.entries[0];
        self.emit_address_high(5, first_function);
        self.emit_address_high(4, global);
        self.record_relocation(RelocationKind::Addr16Lo, first_function);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 5,
            immediate: 0,
        });
        self.record_relocation(RelocationKind::Addr16Lo, global);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 4,
            immediate: 0,
        });

        let mut pending_offset = first_offset;
        for &(offset, function) in &arm.entries[1..] {
            self.emit_address_high(4, function);
            self.output.instructions.push(Instruction::StoreWord {
                s: 0,
                a: 5,
                offset: pending_offset,
            });
            self.record_relocation(RelocationKind::Addr16Lo, function);
            self.output.instructions.push(Instruction::AddImmediate {
                d: 0,
                a: 4,
                immediate: 0,
            });
            pending_offset = offset;
        }
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 5,
            offset: pending_offset,
        });
    }
}

fn recognize(function: &Function) -> Option<FunctionTableSelection<'_>> {
    if !matches!(function.return_type, Type::Int | Type::UnsignedInt)
        || function.parameters.len() != 1
        || function.locals.len() != 1
        || !function.guards.is_empty()
    {
        return None;
    }
    let result = &function.locals[0];
    if result.initializer.is_some()
        || !matches!(
            function.return_expression.as_ref(),
            Some(Expression::Variable(name)) if name == &result.name
        )
    {
        return None;
    }
    let [Statement::If {
        condition,
        then_body,
        else_body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    let selector = match condition {
        Expression::Binary {
            operator: BinaryOperator::Equal,
            left,
            right,
        } if matches!(left.as_ref(), Expression::Variable(name) if name == &function.parameters[0].name) => {
            i16::try_from(constant_value(right)?).ok()?
        }
        _ => return None,
    };
    let (then_global, then_arm) = recognize_arm(then_body, &result.name)?;
    let (else_global, else_arm) = recognize_arm(else_body, &result.name)?;
    if then_global != else_global
        || then_arm.entries.len() < 2
        || then_arm.entries.iter().map(|(offset, _)| offset).ne(
            else_arm.entries.iter().map(|(offset, _)| offset),
        )
    {
        return None;
    }
    Some(FunctionTableSelection {
        selector,
        global: then_global,
        then_arm,
        else_arm,
    })
}

fn recognize_arm<'a>(
    statements: &'a [Statement],
    result: &str,
) -> Option<(&'a str, FunctionTableArm<'a>)> {
    let (first, stores) = statements.split_first()?;
    let Statement::Assign {
        name,
        value: Expression::Call { name: probe, arguments },
    } = first
    else {
        return None;
    };
    if name != result || !arguments.is_empty() {
        return None;
    }
    let mut global = None;
    let mut entries = Vec::with_capacity(stores.len());
    for statement in stores {
        let Statement::Store {
            target:
                Expression::Member {
                    base,
                    offset,
                    index_stride: None,
                    ..
                },
            value: Expression::Variable(function),
        } = statement
        else {
            return None;
        };
        let Expression::Variable(base) = base.as_ref() else {
            return None;
        };
        if global.is_some_and(|global| global != base) {
            return None;
        }
        global = Some(base.as_str());
        entries.push((i16::try_from(*offset).ok()?, function.as_str()));
    }
    if entries.windows(2).any(|pair| pair[0].0 >= pair[1].0) {
        return None;
    }
    Some((
        global?,
        FunctionTableArm {
            probe,
            entries,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{LocalDeclaration, Parameter};

    fn store(global: &str, offset: u32, function: &str) -> Statement {
        Statement::Store {
            target: Expression::Member {
                base: Box::new(Expression::Variable(global.into())),
                offset,
                member_type: Type::Pointer(Pointee::Int),
                index_stride: None,
            },
            value: Expression::Variable(function.into()),
        }
    }

    #[test]
    fn recognizes_two_function_table_arms() {
        let arm = |probe: &str, prefix: &str| {
            vec![
                Statement::Assign {
                    name: "result".into(),
                    value: Expression::Call {
                        name: probe.into(),
                        arguments: vec![],
                    },
                },
                store("table", 0, &format!("{prefix}Open")),
                store("table", 4, &format!("{prefix}Close")),
            ]
        };
        let function = Function {
            return_type: Type::Int,
            name: "select".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![Parameter {
                parameter_type: Type::Int,
                name: "kind".into(),
            }],
            locals: vec![LocalDeclaration {
                declared_type: Type::Int,
                name: "result".into(),
                initializer: None,
                is_volatile: false,
                array_length: None,
                is_static: false,
                data_bytes: None,
                data_relocations: vec![],
                is_const: false,
                row_bytes: None,
            }],
            statements: vec![Statement::If {
                condition: Expression::Binary {
                    operator: BinaryOperator::Equal,
                    left: Box::new(Expression::Variable("kind".into())),
                    right: Box::new(Expression::IntegerLiteral(1)),
                },
                then_body: arm("probe_a", "A"),
                else_body: arm("probe_b", "B"),
            }],
            guards: vec![],
            return_expression: Some(Expression::Variable("result".into())),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: vec![],
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };

        let plan = recognize(&function).expect("the two table arms should be recognized");
        assert_eq!(plan.global, "table");
        assert_eq!(plan.then_arm.entries.len(), 2);
        assert_eq!(plan.else_arm.entries.len(), 2);
    }
}
