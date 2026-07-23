//! Unoptimized local-select allocation and control flow.
//!
//! At `-O0`, mwcc preserves source register locals instead of dissolving an
//! `if/else` assignment into an optimized select. Locals receive descending
//! callee-saved homes even in a leaf function, so this family owns its frame,
//! bindings, diamond, and merge as one allocation unit.

#[allow(unused_imports)]
use super::*;

use mwcc_syntax_trees::Parameter;
use mwcc_versions::Optimization;

#[derive(Clone, Debug)]
pub(crate) struct UnoptimizedLocalSelectSummary {
    pub(crate) parameters: Vec<Parameter>,
    pub(crate) result_name: String,
    pub(crate) result_type: Type,
    pub(crate) derived_name: String,
    pub(crate) derived_type: Type,
    pub(crate) derived_initializer: Expression,
    pub(crate) condition: Expression,
    pub(crate) when_true: Expression,
    pub(crate) when_false: Expression,
}

fn assigned_value<'a>(body: &'a [Statement], result: &str) -> Option<&'a Expression> {
    match body {
        [Statement::Assign { name, value }] if name == result => Some(value),
        _ => None,
    }
}

pub(crate) fn summarize_unoptimized_local_select(
    function: &Function,
) -> Option<UnoptimizedLocalSelectSummary> {
    if !function.guards.is_empty() || function_makes_call(function) {
        return None;
    }
    let [result, derived] = function.locals.as_slice() else {
        return None;
    };
    if result.initializer.is_some()
        || result.array_length.is_some()
        || result.is_static
        || derived.initializer.is_none()
        || derived.array_length.is_some()
        || derived.is_static
        || result.declared_type.width() != 32
        || derived.declared_type.width() != 32
        || !matches!(
            result.declared_type,
            Type::Pointer(_) | Type::StructPointer { .. }
        )
        || !matches!(derived.declared_type, Type::Int | Type::UnsignedInt)
        || function.return_type != result.declared_type
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
    let when_true = assigned_value(then_body, &result.name)?;
    let when_false = assigned_value(else_body, &result.name)?;
    Some(UnoptimizedLocalSelectSummary {
        parameters: function.parameters.clone(),
        result_name: result.name.clone(),
        result_type: result.declared_type,
        derived_name: derived.name.clone(),
        derived_type: derived.declared_type,
        derived_initializer: derived.initializer.clone().expect("classified"),
        condition: condition.clone(),
        when_true: when_true.clone(),
        when_false: when_false.clone(),
    })
}

impl Generator {
    pub(crate) fn bind_unoptimized_local_select_value(
        &mut self,
        name: &str,
        value_type: Type,
        register: u8,
    ) {
        let pointee = match value_type {
            Type::Pointer(pointee) => Some(pointee),
            _ => None,
        };
        self.locations.insert(
            name.to_string(),
            Location {
                class: ValueClass::General,
                register,
                signed: self.signed_of(value_type),
                width: value_type.width(),
                pointee,
                stride: pointer_stride(value_type),
            },
        );
    }

    /// Assign one branch value into a source local's callee-saved home. An
    /// array decay keeps its short-lived address high half in the lowest
    /// volatile and writes the completed pointer into the local home.
    fn emit_unoptimized_local_select_arm(
        &mut self,
        value: &Expression,
        value_type: Type,
        destination: u8,
    ) -> Compilation<()> {
        if let Expression::Variable(name) = value {
            if self.global_array_sizes.contains_key(name.as_str()) {
                let address = self.lowest_free_general()?;
                self.emit_address_high(address, name);
                self.record_relocation(RelocationKind::Addr16Lo, name);
                self.output.instructions.push(Instruction::AddImmediate {
                    d: destination,
                    a: address,
                    immediate: 0,
                });
                return Ok(());
            }
        }
        self.evaluate(value, value_type, destination)
    }

    /// `-O0` preserves a non-zero masked equality as three explicit values:
    /// mask into a volatile, subtract the comparison constant into r0, then an
    /// unsigned zero test. Optimized levels use different select scheduling and
    /// never route through this owner.
    fn emit_unoptimized_masked_condition(
        &mut self,
        condition: &Expression,
    ) -> Compilation<Option<(u8, u8)>> {
        let Expression::Binary {
            operator: comparison @ (BinaryOperator::Equal | BinaryOperator::NotEqual),
            left,
            right,
        } = condition
        else {
            return Ok(None);
        };
        let (masked, constant) = if let Some(constant) = constant_value(right) {
            (left.as_ref(), constant)
        } else if let Some(constant) = constant_value(left) {
            (right.as_ref(), constant)
        } else {
            return Ok(None);
        };
        let Expression::Binary {
            operator: BinaryOperator::BitAnd,
            left: value,
            right: mask_expression,
        } = masked
        else {
            return Ok(None);
        };
        let Some(mask) = constant_value(mask_expression) else {
            return Ok(None);
        };
        let (Some(source), Some((begin, end))) = (
            leaf_name(value).and_then(|name| self.lookup_general(name)),
            mask_to_run(mask as u32),
        ) else {
            return Ok(None);
        };
        if constant & !mask != 0 {
            return Ok(None);
        }

        let masked_register = self.lowest_free_general()?;
        self.output.instructions.push(Instruction::RotateAndMask {
            a: masked_register,
            s: source,
            shift: 0,
            begin,
            end,
        });
        let negated = constant.wrapping_neg();
        if negated & 0xffff == 0 {
            let immediate = i16::try_from(negated >> 16).map_err(|_| {
                Diagnostic::error("masked comparison constant is not addis-encodable")
            })?;
            self.output
                .instructions
                .push(Instruction::AddImmediateShifted {
                    d: GENERAL_SCRATCH,
                    a: masked_register,
                    immediate,
                });
        } else {
            let immediate = i16::try_from(negated).map_err(|_| {
                Diagnostic::error("masked comparison constant is not addi-encodable")
            })?;
            self.output.instructions.push(Instruction::AddImmediate {
                d: GENERAL_SCRATCH,
                a: masked_register,
                immediate,
            });
        }
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: GENERAL_SCRATCH,
                immediate: 0,
            });
        // The returned condition describes the FALSE edge of the source test.
        Ok(Some(match comparison {
            BinaryOperator::Equal => (4, 2),     // bne
            BinaryOperator::NotEqual => (12, 2), // beq
            _ => unreachable!(),
        }))
    }

    /// Emit only the selector's values and control-flow diamond. The standalone
    /// helper and a caller that MWCC expands inline deliberately share this
    /// core, while each owner remains responsible for its own frame and local
    /// copy chain.
    pub(crate) fn emit_unoptimized_local_select_core(
        &mut self,
        plan: &UnoptimizedLocalSelectSummary,
        result_home: u8,
        derived_home: u8,
    ) -> Compilation<()> {
        self.bind_unoptimized_local_select_value(&plan.result_name, plan.result_type, result_home);
        self.bind_unoptimized_local_select_value(
            &plan.derived_name,
            plan.derived_type,
            derived_home,
        );

        self.evaluate(&plan.derived_initializer, plan.derived_type, derived_home)
            .map_err(|error| {
                Diagnostic::error(format!("unoptimized local-select derived value: {error}"))
            })?;
        let (options, condition_bit) =
            match self.emit_unoptimized_masked_condition(&plan.condition)? {
                Some(branch) => branch,
                None => self.emit_condition_test(&plan.condition).map_err(|error| {
                    Diagnostic::error(format!("unoptimized local-select condition: {error}"))
                })?,
            };
        let else_label = self.fresh_label();
        let join_label = self.fresh_label();
        self.emit_branch_conditional_to(options, condition_bit, else_label);
        self.emit_unoptimized_local_select_arm(&plan.when_true, plan.result_type, result_home)
            .map_err(|error| {
                Diagnostic::error(format!("unoptimized local-select true arm: {error}"))
            })?;
        self.emit_branch_to(join_label);
        self.bind_label(else_label);
        self.emit_unoptimized_local_select_arm(&plan.when_false, plan.result_type, result_home)
            .map_err(|error| {
                Diagnostic::error(format!("unoptimized local-select false arm: {error}"))
            })?;
        self.bind_label(join_label);
        Ok(())
    }

    /// Emit the measured `-O0` two-local pointer select:
    ///
    /// `T *result; int derived = EXPR; if (COND) result = A; else result = B;
    /// return result;`
    ///
    /// Source declaration order owns r31 then r30. The leaf still receives a
    /// 16-byte frame, and both arms join through the r31 result local before the
    /// shared restore sequence.
    pub(crate) fn try_unoptimized_local_select(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.optimization != Optimization::O0 {
            return Ok(false);
        }
        let Some(plan) = summarize_unoptimized_local_select(function) else {
            return Ok(false);
        };

        const RESULT_HOME: u8 = 31;
        const DERIVED_HOME: u8 = 30;
        self.output.pre_scheduled = true;
        self.output.anonymous_label_bump += 2;
        self.frame_size = 16;
        self.callee_saved = vec![RESULT_HOME, DERIVED_HOME];
        self.reserved.extend([RESULT_HOME, DERIVED_HOME]);
        // `-O0` retains incoming parameter homes for the whole source function;
        // short-lived mask/address values begin after them even once a path no
        // longer reads the parameter.
        let parameter_homes: Vec<u8> = function
            .parameters
            .iter()
            .filter_map(|parameter| self.lookup_general(&parameter.name))
            .collect();
        self.reserved.extend(parameter_homes);

        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: RESULT_HOME,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: DERIVED_HOME,
            a: 1,
            offset: 8,
        });

        self.emit_unoptimized_local_select_core(&plan, RESULT_HOME, DERIVED_HOME)?;
        self.output.instructions.push(Instruction::move_register(
            Eabi::general_result().number,
            RESULT_HOME,
        ));
        self.emit_epilogue_and_return();
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::Parameter;

    fn pointer() -> Type {
        Type::Pointer(Pointee::UnsignedChar)
    }

    fn sample() -> Function {
        Function {
            return_type: pointer(),
            name: "select".into(),
            is_static: true,
            is_weak: false,
            parameters: vec![Parameter {
                parameter_type: Type::UnsignedInt,
                name: "input".into(),
            }],
            locals: vec![
                LocalDeclaration {
                    declared_type: pointer(),
                    name: "result".into(),
                    initializer: None,
                    is_volatile: false,
                    array_length: None,
                    is_static: false,
                    data_bytes: None,
                    data_relocations: Vec::new(),
                    is_const: false,
                    row_bytes: None,
                },
                LocalDeclaration {
                    declared_type: Type::UnsignedInt,
                    name: "derived".into(),
                    initializer: Some(Expression::Binary {
                        operator: BinaryOperator::ShiftRight,
                        left: Box::new(Expression::Variable("input".into())),
                        right: Box::new(Expression::IntegerLiteral(16)),
                    }),
                    is_volatile: false,
                    array_length: None,
                    is_static: false,
                    data_bytes: None,
                    data_relocations: Vec::new(),
                    is_const: false,
                    row_bytes: None,
                },
            ],
            statements: vec![Statement::If {
                condition: Expression::Variable("input".into()),
                then_body: vec![Statement::Assign {
                    name: "result".into(),
                    value: Expression::Variable("a".into()),
                }],
                else_body: vec![Statement::Assign {
                    name: "result".into(),
                    value: Expression::Variable("b".into()),
                }],
            }],
            guards: Vec::new(),
            return_expression: Some(Expression::Variable("result".into())),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        }
    }

    #[test]
    fn recognizes_two_source_locals_without_using_function_names() {
        let function = sample();
        let plan = summarize_unoptimized_local_select(&function).expect("recognized");
        assert_eq!(plan.result_name, "result");
        assert_eq!(plan.derived_name, "derived");
    }

    #[test]
    fn rejects_an_arm_that_assigns_another_value() {
        let mut function = sample();
        let Statement::If { then_body, .. } = &mut function.statements[0] else {
            unreachable!()
        };
        let Statement::Assign { name, .. } = &mut then_body[0] else {
            unreachable!()
        };
        *name = "derived".into();
        assert!(summarize_unoptimized_local_select(&function).is_none());
    }
}
