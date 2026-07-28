//! Version-specific anonymous-symbol accounting after instruction selection.
//!
//! GC 4.1 exposes optimizer bookkeeping nodes through otherwise-unused `@N`
//! ordinals. They do not change instructions, so keep their structural matching
//! out of body lowering and apply the measured cost to the function's trailing
//! ordinal block here.

use mwcc_machine_code::MachineFunction;
use mwcc_syntax_trees::{
    BinaryOperator, Expression, Function, Statement, Type, UnaryOperator,
};
use mwcc_versions::FunctionOrdinalAccountingStyle;

pub(crate) fn apply(
    function: &Function,
    output: &mut MachineFunction,
    style: FunctionOrdinalAccountingStyle,
) {
    let hidden = match style {
        FunctionOrdinalAccountingStyle::Mainline => {
            mainline_initialized_array_labels(function, output)
                + mainline_variadic_float_conversion_labels(function)
                + mainline_call_ladder_labels(function)
        }
        FunctionOrdinalAccountingStyle::Gc41 => gc41_hidden_labels(function, false),
        FunctionOrdinalAccountingStyle::Gc41Ipa => gc41_hidden_labels(function, true),
    };
    output.post_constant_label_bump += hidden;
}

/// The mainline optimizer turns a run of zero-initialized automatic arrays
/// into one pooled image per array. Each copy contributes one internal label,
/// with one shared label closing the run. Lone-array pooling is handled by its
/// dedicated lowering path and does not enter this multi-image accounting.
fn mainline_initialized_array_labels(function: &Function, output: &mut MachineFunction) -> u32 {
    let retained_dead_arrays = function
        .locals
        .iter()
        .filter(|local| {
            !local.is_static
                && local.is_const
                && local.array_length.is_some()
                && local
                    .data_bytes
                    .as_ref()
                    .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
                && !crate::analysis::function_uses_name(function, &local.name)
        })
        .count() as u32;
    // The visible images are attached after executable lowering. MWCC also
    // leaves one hidden trailing label per image and one shared terminator.
    let retained_dead_labels = retained_dead_arrays + u32::from(retained_dead_arrays != 0);

    let pooled_zero_arrays = function
        .locals
        .iter()
        .filter(|local| {
            !local.is_static
                && local.array_length.is_some()
                && local
                    .data_bytes
                    .as_ref()
                    .is_some_and(|bytes| !bytes.is_empty() && bytes.iter().all(|byte| *byte == 0))
                && !(local.is_const
                    && !crate::analysis::function_uses_name(function, &local.name))
        })
        .count() as u32;
    let emitted_pooled_images = pooled_zero_arrays >= 2
        && output.anonymous_rodata.len() >= pooled_zero_arrays as usize;
    if emitted_pooled_images {
        output.anonymous_rodata[0].static_slot_prefix_bump =
            Some(output.object_anonymous_bump());
    }
    retained_dead_labels
        + if pooled_zero_arrays < 2 {
            0
        } else if emitted_pooled_images {
            // The writer walks the N concrete image symbols. One internal label
            // per copy and the shared closing label remain hidden. The object
            // boundary preserves any function-front labels displaced by the
            // first image's explicit static-slot placement.
            pooled_zero_arrays + 1
        } else {
            3 * pooled_zero_arrays + 1
        }
}

/// A variadic call with multiple floating-to-integer arguments keeps one
/// optimizer bookkeeping label for every conversion after the first. This is
/// observable in the next static-local ordinal even though the labels never
/// reach the instruction stream.
fn mainline_variadic_float_conversion_labels(function: &Function) -> u32 {
    function
        .statements
        .iter()
        .map(|statement| match statement {
            Statement::Expression(Expression::Call { arguments, .. }) => arguments
                .iter()
                .filter(|argument| is_float_to_integer_cast(argument))
                .count()
                .saturating_sub(1) as u32,
            _ => 0,
        })
        .sum()
}

fn is_float_to_integer_cast(expression: &Expression) -> bool {
    let Expression::Cast {
        target_type,
        operand,
    } = expression
    else {
        return false;
    };
    matches!(
        target_type,
        Type::Int
            | Type::UnsignedInt
            | Type::Short
            | Type::UnsignedShort
            | Type::Char
            | Type::UnsignedChar
    ) && matches!(
        operand.as_ref(),
        Expression::Member {
            member_type: Type::Float | Type::Double,
            ..
        }
    )
}

/// A complete call-based if/else ladder retains one label per condition plus
/// the shared join. Specialized control-flow owners account their labels while
/// emitting; this covers the ordinary structured-body path that otherwise has
/// no visible label objects to number.
fn mainline_call_ladder_labels(function: &Function) -> u32 {
    // Automatic-local ladders are owned by specialized body lowerers which
    // already account their control labels while assigning registers.
    if function.locals.iter().any(|local| !local.is_static) {
        return 0;
    }
    let [statement] = function.statements.as_slice() else {
        return 0;
    };
    call_ladder_depth(statement).map_or(0, |depth| depth + 1)
}

fn call_ladder_depth(statement: &Statement) -> Option<u32> {
    let Statement::If {
        then_body,
        else_body,
        ..
    } = statement
    else {
        return None;
    };
    if !is_single_call_body(then_body) {
        return None;
    }
    if is_single_call_body(else_body) {
        Some(1)
    } else if let [nested] = else_body.as_slice() {
        call_ladder_depth(nested).map(|depth| depth + 1)
    } else {
        None
    }
}

fn is_single_call_body(statements: &[Statement]) -> bool {
    matches!(
        statements,
        [Statement::Expression(
            Expression::Call { .. } | Expression::CallThrough { .. }
        )]
    )
}

pub(crate) fn apply_unit(
    functions: &[Function],
    machine_functions: &mut [MachineFunction],
    style: FunctionOrdinalAccountingStyle,
) {
    if style != FunctionOrdinalAccountingStyle::Gc41Ipa || machine_functions.is_empty() {
        return;
    }

    let mut saw_float_guard_pool = false;
    let mut unit_front_bump = 0u32;
    for function in functions {
        let Some(machine) = machine_functions
            .iter_mut()
            .find(|machine| machine.name == function.name)
        else {
            continue;
        };
        let has_float_guard_pool = function
            .guards
            .iter()
            .any(|guard| is_float_comparison(&guard.condition))
            && !machine.constants.is_empty();
        if has_float_guard_pool {
            if saw_float_guard_pool {
                // Later pool-bearing float guards are analyzed before pool
                // allocation (+7 at unit front), while three of their four
                // local guard labels coalesce into that unit analysis block.
                unit_front_bump += 7;
                machine.anonymous_label_bump = machine.anonymous_label_bump.saturating_sub(3);
            }
            saw_float_guard_pool = true;
        }
        if function
            .guards
            .iter()
            .any(|guard| is_negated_call_short_circuit(&guard.condition))
        {
            unit_front_bump += 16;
        }
    }
    machine_functions[0].anonymous_label_bump += unit_front_bump;
}

fn gc41_hidden_labels(function: &Function, ipa_file: bool) -> u32 {
    if let [Statement::Store { value, .. }] = function.statements.as_slice() {
        return if matches!(value, Expression::Variable(_)) {
            6
        } else {
            5
        };
    }

    if function.guards.len() == 1 && function.return_expression.is_some() {
        if is_float_comparison(&function.guards[0].condition) {
            // Under file IPA, these trailing nodes join the unit-wide pool-front
            // block instead of remaining after this function's constant.
            return if ipa_file { 0 } else { 4 };
        }
        return 7 + u32::from(ipa_file);
    }

    if let Some(expression) = &function.return_expression {
        if is_float_comparison(expression) {
            return if ipa_file { 0 } else { 3 };
        }
        if is_comparison(expression) {
            return 5;
        }
    }
    0
}

fn is_comparison(expression: &Expression) -> bool {
    matches!(expression, Expression::Binary { operator, .. }
        if crate::analysis::is_comparison(*operator))
}

fn is_float_comparison(expression: &Expression) -> bool {
    matches!(expression, Expression::Binary { operator, left, right }
        if crate::analysis::is_comparison(*operator)
            && (is_float_value(left) || is_float_value(right)))
}

fn is_float_value(expression: &Expression) -> bool {
    match expression {
        Expression::FloatLiteral(_) => true,
        Expression::Cast { target_type, .. } => {
            matches!(target_type, Type::Float | Type::Double)
        }
        _ => false,
    }
}

fn is_negated_call_short_circuit(expression: &Expression) -> bool {
    let Expression::Binary {
        operator: BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr,
        left,
        right,
    } = expression
    else {
        return false;
    };
    is_negated_call(left) && is_negated_call(right)
}

fn is_negated_call(expression: &Expression) -> bool {
    matches!(expression, Expression::Unary {
        operator: UnaryOperator::LogicalNot,
        operand,
    } if matches!(operand.as_ref(), Expression::Call { .. }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{BinaryOperator, GuardedReturn};

    fn function() -> Function {
        Function {
            return_type: Type::Int,
            name: "probe".to_string(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: Vec::new(),
            statements: Vec::new(),
            guards: Vec::new(),
            return_expression: None,
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
    fn gc41_integer_guard_cost_is_ipa_sensitive() {
        let mut function = function();
        function.guards.push(GuardedReturn {
            condition: Expression::Binary {
                operator: BinaryOperator::Less,
                left: Box::new(Expression::Variable("value".to_string())),
                right: Box::new(Expression::IntegerLiteral(256)),
            },
            value: Expression::IntegerLiteral(1),
        });
        function.return_expression = Some(Expression::IntegerLiteral(0));
        assert_eq!(gc41_hidden_labels(&function, false), 7);
        assert_eq!(gc41_hidden_labels(&function, true), 8);
    }

    #[test]
    fn mainline_accounts_a_shared_run_of_zero_array_images() {
        let mut function = function();
        for (name, size) in [("date", 32), ("time", 32), ("ampm", 32), ("scratch", 256)] {
            function.locals.push(mwcc_syntax_trees::LocalDeclaration {
                declared_type: Type::Char,
                name: name.to_owned(),
                initializer: None,
                is_volatile: false,
                array_length: Some(size),
                is_static: false,
                data_bytes: Some(vec![0; usize::from(size)]),
                data_relocations: Vec::new(),
                is_const: false,
                row_bytes: None,
            });
        }
        let mut output = MachineFunction::new("probe");
        output.anonymous_label_bump = 8;
        for size in [32, 32, 32, 256] {
            output
                .anonymous_rodata
                .push(mwcc_machine_code::AnonymousRodata {
                    bytes: vec![0; size],
                    static_slot_prefix_bump: None,
                    anonymous_offset: 0,
                });
        }
        output.anonymous_rodata[0].static_slot_prefix_bump = Some(0);
        apply(
            &function,
            &mut output,
            FunctionOrdinalAccountingStyle::Mainline,
        );
        assert_eq!(
            output.anonymous_rodata[0].static_slot_prefix_bump,
            Some(8)
        );
        assert_eq!(output.post_constant_label_bump, 5);
    }

    #[test]
    fn mainline_accounts_retained_dead_const_array_images() {
        for (array_count, expected_bump) in [(1, 2), (2, 3), (3, 4)] {
            let mut function = function();
            for index in 0..array_count {
                function.locals.push(mwcc_syntax_trees::LocalDeclaration {
                    declared_type: Type::Char,
                    name: format!("unused_{index}"),
                    initializer: None,
                    is_volatile: false,
                    array_length: Some(1),
                    is_static: false,
                    data_bytes: Some(Vec::new()),
                    data_relocations: Vec::new(),
                    is_const: true,
                    row_bytes: None,
                });
            }
            let mut output = MachineFunction::new("probe");
            apply(
                &function,
                &mut output,
                FunctionOrdinalAccountingStyle::Mainline,
            );
            assert_eq!(
                output.post_constant_label_bump, expected_bump,
                "{array_count} retained dead array(s)"
            );
        }
    }

    #[test]
    fn mainline_accounts_additional_variadic_float_conversions() {
        let float_member = || Expression::Member {
            base: Box::new(Expression::Variable("value".to_owned())),
            offset: 0,
            member_type: Type::Float,
            index_stride: None,
        };
        let converted = || Expression::Cast {
            target_type: Type::Int,
            operand: Box::new(float_member()),
        };
        let mut function = function();
        function
            .statements
            .push(Statement::Expression(Expression::Call {
                name: "sprintf".to_owned(),
                arguments: vec![
                    Expression::Variable("buffer".to_owned()),
                    Expression::StringLiteral(b"%d,%d,%d".to_vec()),
                    converted(),
                    converted(),
                    converted(),
                ],
            }));
        assert_eq!(mainline_variadic_float_conversion_labels(&function), 2);
    }

    #[test]
    fn mainline_accounts_a_nested_call_ladder_and_join() {
        let call = |name: &str| {
            Statement::Expression(Expression::Call {
                name: name.to_owned(),
                arguments: Vec::new(),
            })
        };
        let mut function = function();
        function.statements.push(Statement::If {
            condition: Expression::Variable("first".to_owned()),
            then_body: vec![call("zero")],
            else_body: vec![Statement::If {
                condition: Expression::Variable("second".to_owned()),
                then_body: vec![call("one")],
                else_body: vec![call("many")],
            }],
        });
        assert_eq!(mainline_call_ladder_labels(&function), 3);

        function.locals.push(mwcc_syntax_trees::LocalDeclaration {
            declared_type: Type::Int,
            name: "saved_condition".to_owned(),
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            row_bytes: None,
        });
        assert_eq!(mainline_call_ladder_labels(&function), 0);
    }

    #[test]
    fn gc41_ipa_moves_float_guard_trailing_labels_out_of_function() {
        let mut function = function();
        function.guards.push(GuardedReturn {
            condition: Expression::Binary {
                operator: BinaryOperator::GreaterEqual,
                left: Box::new(Expression::Variable("value".to_string())),
                right: Box::new(Expression::FloatLiteral(0.5)),
            },
            value: Expression::IntegerLiteral(1),
        });
        function.return_expression = Some(Expression::IntegerLiteral(0));
        assert_eq!(gc41_hidden_labels(&function, false), 4);
        assert_eq!(gc41_hidden_labels(&function, true), 0);
    }

    #[test]
    fn gc41_ipa_accounts_later_float_pool_and_short_circuit_at_unit_front() {
        let mut ground = function();
        ground.name = "ground".to_string();
        ground.guards.push(GuardedReturn {
            condition: Expression::Binary {
                operator: BinaryOperator::GreaterEqual,
                left: Box::new(Expression::Variable("value".to_string())),
                right: Box::new(Expression::FloatLiteral(0.5)),
            },
            value: Expression::IntegerLiteral(1),
        });
        ground.return_expression = Some(Expression::IntegerLiteral(0));

        let mut roof = ground.clone();
        roof.name = "roof".to_string();

        let mut wall = function();
        wall.name = "wall".to_string();
        let call = |name: &str| Expression::Unary {
            operator: UnaryOperator::LogicalNot,
            operand: Box::new(Expression::Call {
                name: name.to_string(),
                arguments: Vec::new(),
            }),
        };
        wall.guards.push(GuardedReturn {
            condition: Expression::Binary {
                operator: BinaryOperator::LogicalAnd,
                left: Box::new(call("ground")),
                right: Box::new(call("roof")),
            },
            value: Expression::IntegerLiteral(1),
        });
        wall.return_expression = Some(Expression::IntegerLiteral(0));

        let mut machines = vec![
            MachineFunction::new("ground"),
            MachineFunction::new("roof"),
            MachineFunction::new("wall"),
        ];
        let pool = |bits| mwcc_machine_code::PoolConstant {
            bits,
            byte_width: 4,
            static_slot: false,
            image: false,
            force_new: false,
        };
        machines[0].constants.push(pool(0x3f00_0000));
        machines[0].anonymous_label_bump = 4;
        machines[1].constants.push(pool(0xbf4c_cccd));
        machines[1].anonymous_label_bump = 4;

        apply_unit(
            &[ground, roof, wall],
            &mut machines,
            FunctionOrdinalAccountingStyle::Gc41Ipa,
        );
        assert_eq!(machines[0].anonymous_label_bump, 27);
        assert_eq!(machines[1].anonymous_label_bump, 1);
    }
}
