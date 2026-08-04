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
use mwcc_versions::{Behavior, FunctionOrdinalAccountingStyle};
use std::collections::HashSet;

mod automatic_image_conversion;

/// Build 163 assigns retained inline-initializer nodes after a function's
/// strings. Move those ordinals out of the pool-front block and reinsert them
/// immediately before the first constant. Functions without strings keep the
/// front adjustment only, matching the conversion-owned pool transaction.
pub(crate) fn relocate_inline_initializer_ordinals(
    output: &mut MachineFunction,
    facts: mwcc_syntax_trees::InlineExpansionFacts,
    enabled: bool,
) {
    let moved = facts.leading_initializer_substitutions as u32;
    if !enabled || moved == 0 || output.constants.is_empty() {
        return;
    }
    output.constant_number_adjust -= i32::try_from(moved).unwrap_or(i32::MAX);
    if !output.string_literals.is_empty() {
        output.string_number_adjust -= i32::try_from(
            moved.saturating_sub(u32::from(output.has_conversion)),
        )
        .unwrap_or(i32::MAX);
        match output
            .constant_number_gaps
            .iter_mut()
            .find(|(constant_index, _)| *constant_index == 0)
        {
            Some((_, gap)) => *gap = gap.saturating_add(moved),
            None => output.constant_number_gaps.push((0, moved)),
        }
    }
}

pub(crate) fn apply(
    function: &Function,
    output: &mut MachineFunction,
    style: FunctionOrdinalAccountingStyle,
) {
    let hidden = match style {
        FunctionOrdinalAccountingStyle::Mainline => {
            mainline_initialized_array_labels(function, output)
                + mainline_variadic_float_conversion_labels(function)
                + mainline_call_ladder_labels(function, output)
                + mainline_static_aggregate_initializer_labels(function)
        }
        FunctionOrdinalAccountingStyle::Gc41 => gc41_hidden_labels(function, false),
        FunctionOrdinalAccountingStyle::Gc41Ipa => gc41_hidden_labels(function, true),
    };
    output.post_constant_label_bump += hidden;
}

pub(crate) fn apply_with_behavior(
    function: &Function,
    output: &mut MachineFunction,
    behavior: &Behavior,
) {
    let empty_then_residue = empty_conditional_then_count(&function.statements)
        * u32::from(behavior.empty_conditional_then_label_bump);
    output.anonymous_label_bump += empty_then_residue;
    apply(
        function,
        output,
        behavior.function_ordinal_accounting_style,
    );
}

fn empty_conditional_then_count(statements: &[Statement]) -> u32 {
    statements
        .iter()
        .map(|statement| match statement {
            Statement::If {
                then_body,
                else_body,
                ..
            } => {
                u32::from(then_body.is_empty())
                    + empty_conditional_then_count(then_body)
                    + empty_conditional_then_count(else_body)
            }
            Statement::Loop { body, .. } => empty_conditional_then_count(body),
            Statement::Switch { arms, default, .. } => {
                arms.iter()
                    .map(|arm| match &arm.body {
                        mwcc_syntax_trees::ArmBody::Statements(statements) => {
                            empty_conditional_then_count(statements)
                        }
                        mwcc_syntax_trees::ArmBody::Return(_) => 0,
                    })
                    .sum::<u32>()
                    + default.as_ref().map_or(0, |body| match body {
                        mwcc_syntax_trees::ArmBody::Statements(statements) => {
                            empty_conditional_then_count(statements)
                        }
                        mwcc_syntax_trees::ArmBody::Return(_) => 0,
                    })
            }
            _ => 0,
        })
        .sum()
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
                && crate::analysis::function_uses_name(function, &local.name)
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
            // The writer walks the N concrete image symbols. Selection retains
            // two additional bookkeeping labels per copy transaction; these
            // remain hidden but advance the next source static's ordinal.
            2 * pooled_zero_arrays
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
fn mainline_call_ladder_labels(function: &Function, output: &MachineFunction) -> u32 {
    // Automatic-local ladders are owned by specialized body lowerers which
    // already account their control labels while assigning registers.
    if function.locals.iter().any(|local| !local.is_static) {
        return 0;
    }
    let [statement] = function.statements.as_slice() else {
        return 0;
    };
    call_ladder_depth(statement).map_or(0, |depth| {
        if output.anonymous_label_bump == 0 {
            depth + 1
        } else {
            // A specialized body owner already charged the condition labels;
            // only the common source join remains outside its visible blocks.
            1
        }
    })
}

/// A static aggregate initialized word-by-word contributes one optimizer node
/// per word and one binding node per relocation. Restrict this to the measured
/// single-local registration shape: the declaration's ordinal is assigned
/// before these nodes, while later functions observe them.
fn mainline_static_aggregate_initializer_labels(function: &Function) -> u32 {
    let [local] = function.locals.as_slice() else {
        return 0;
    };
    let Type::Struct { size, .. } = &local.declared_type else {
        return 0;
    };
    if !local.is_static
        || local.array_length.is_some()
        || local.data_bytes.as_ref().map_or(0, Vec::len) != *size as usize
        || local.data_relocations.is_empty()
        || !matches!(
            function.statements.as_slice(),
            [Statement::Expression(Expression::Call { arguments, .. })]
                if arguments.iter().any(|argument| matches!(
                    argument,
                    Expression::AddressOf { operand }
                        if matches!(operand.as_ref(), Expression::Variable(name) if name == &local.name)
                ))
        )
    {
        return 0;
    }
    size.div_ceil(4) + local.data_relocations.len() as u32
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
    apply_deferred_constant_scopes(machine_functions);
    resolve_gc41_automatic_image_slots(functions, machine_functions, style);
    automatic_image_conversion::apply_unit(functions, machine_functions, style);
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

/// Resolve declaration-time automatic images after the unit-wide source-name
/// analysis is known.
///
/// GC 4.1 initially charges every named function parameter to the unit front,
/// although the first automatic image is numbered at its source position. The
/// first such image therefore credits parameters from later definitions in
/// addition to its function-local front labels. The object writer restores a
/// static-slot credit after each function, so every later image in the unit
/// applies that same source-position credit again.
fn resolve_gc41_automatic_image_slots(
    functions: &[Function],
    machine_functions: &mut [MachineFunction],
    style: FunctionOrdinalAccountingStyle,
) {
    if !matches!(
        style,
        FunctionOrdinalAccountingStyle::Gc41 | FunctionOrdinalAccountingStyle::Gc41Ipa
    ) {
        return;
    }

    let source_credit = machine_functions
        .iter()
        .find(|machine| {
            machine
                .anonymous_rodata
                .iter()
                .any(|blob| blob.static_slot_prefix_bump.is_some())
        })
        .and_then(|machine| {
            functions
                .iter()
                .position(|function| function.name == machine.name)
        })
        .map_or(0, |source_index| {
            functions[source_index + 1..]
                .iter()
                .map(|function| function.parameters.len() as u32)
                .sum()
        });
    for machine in machine_functions {
        if !machine
            .anonymous_rodata
            .iter()
            .any(|blob| blob.static_slot_prefix_bump.is_some())
        {
            continue;
        }
        let front_credit = machine.object_anonymous_bump().saturating_sub(1);
        if let Some(blob) = machine
            .anonymous_rodata
            .iter_mut()
            .find(|blob| blob.static_slot_prefix_bump.is_some())
        {
            blob.static_slot_prefix_bump = Some(front_credit + source_credit);
        }
    }
}

fn apply_deferred_constant_scopes(machine_functions: &mut [MachineFunction]) {
    let mut numbered = HashSet::new();
    let mut pending = 0u32;
    for machine in machine_functions {
        let introduces_constant = machine.constants.iter().any(|constant| {
            constant.force_new || !numbered.contains(&(constant.bits, constant.byte_width))
        });
        if pending != 0 && introduces_constant {
            machine.constant_number_adjust = machine
                .constant_number_adjust
                .saturating_add(i32::try_from(pending).unwrap_or(i32::MAX));
            machine.post_function_counter_rollback = machine
                .post_function_counter_rollback
                .saturating_add(pending);
            pending = 0;
        }
        for constant in &machine.constants {
            if !constant.force_new {
                numbered.insert((constant.bits, constant.byte_width));
            }
        }
        pending = pending.saturating_add(machine.deferred_next_constant_scope_bump);
    }
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
    use mwcc_syntax_trees::{
        BinaryOperator, GuardedReturn, LocalDataRelocation, LocalDataRelocationTarget,
    };
    use mwcc_versions::{CompilerConfig, GC_1_2_5N};

    pub(super) fn function() -> Function {
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

    fn source_slot(bytes: usize) -> mwcc_machine_code::AnonymousRodata {
        mwcc_machine_code::AnonymousRodata {
            bytes: vec![0; bytes],
            comment_alignment: 4,
            static_slot_prefix_bump: Some(0),
            anonymous_offset: 0,
        }
    }

    #[test]
    fn gc41_source_slots_credit_later_function_parameter_analysis() {
        let mut first = function();
        first.name = "first".into();
        let mut second = function();
        second.name = "second".into();
        second.parameters.push(mwcc_syntax_trees::Parameter {
            parameter_type: Type::Int,
            name: "value".into(),
        });

        let mut first_machine = MachineFunction::new("first");
        first_machine.anonymous_label_bump = 3;
        first_machine.anonymous_rodata.push(source_slot(16));
        let mut second_machine = MachineFunction::new("second");
        second_machine.anonymous_label_bump = 3;
        second_machine.anonymous_rodata.push(source_slot(16));
        let mut machines = [first_machine, second_machine];

        resolve_gc41_automatic_image_slots(
            &[first, second],
            &mut machines,
            FunctionOrdinalAccountingStyle::Gc41,
        );

        assert_eq!(machines[0].anonymous_rodata[0].static_slot_prefix_bump, Some(3));
        assert_eq!(machines[1].anonymous_rodata[0].static_slot_prefix_bump, Some(3));
    }

    #[test]
    fn build_163_moves_inline_initializer_ordinals_behind_strings() {
        let mut output = MachineFunction::new("probe");
        output.string_literals.push(b"assert.c".to_vec());
        output.intern_constant(0x4330_0000_0000_0000, 8);
        output.has_conversion = true;

        relocate_inline_initializer_ordinals(
            &mut output,
            mwcc_syntax_trees::InlineExpansionFacts {
                leading_initializer_substitutions: 2,
                body_value_substitutions: 0,
            },
            true,
        );

        assert_eq!(output.constant_number_adjust, -2);
        assert_eq!(output.string_number_adjust, -1);
        assert_eq!(output.constant_number_gaps, [(0, 2)]);
    }

    #[test]
    fn build_163_pool_without_strings_keeps_the_initializer_discount_at_front() {
        let mut output = MachineFunction::new("probe");
        output.intern_constant(0x4330_0000_0000_0000, 8);

        relocate_inline_initializer_ordinals(
            &mut output,
            mwcc_syntax_trees::InlineExpansionFacts {
                leading_initializer_substitutions: 1,
                body_value_substitutions: 0,
            },
            true,
        );

        assert_eq!(output.constant_number_adjust, -1);
        assert_eq!(output.string_number_adjust, 0);
        assert!(output.constant_number_gaps.is_empty());
    }

    #[test]
    fn build_163_retains_an_empty_then_residue_at_pool_front() {
        let mut function = function();
        function.statements.push(Statement::If {
            condition: Expression::Variable("condition".to_owned()),
            then_body: Vec::new(),
            else_body: vec![Statement::Return(Some(Expression::IntegerLiteral(1)))],
        });
        let mut output = MachineFunction::new("probe");
        let behavior = Behavior::resolve(&CompilerConfig::new(GC_1_2_5N));

        apply_with_behavior(&function, &mut output, &behavior);

        assert_eq!(output.anonymous_label_bump, 1);
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
                attribute_alignment: None,
                row_bytes: None,
            });
            function
                .statements
                .push(Statement::Expression(Expression::Call {
                    name: "consume".to_owned(),
                    arguments: vec![Expression::Variable(name.to_owned())],
                }));
        }
        let mut output = MachineFunction::new("probe");
        output.anonymous_label_bump = 8;
        for size in [32, 32, 32, 256] {
            output
                .anonymous_rodata
                .push(mwcc_machine_code::AnonymousRodata {
                    bytes: vec![0; size],
                    comment_alignment: 4,
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
        assert_eq!(output.post_constant_label_bump, 8);
    }

    #[test]
    fn mainline_accounts_retained_dead_mutable_array_images() {
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
                    is_const: false,
                    attribute_alignment: None,
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
        let mut output = MachineFunction::new("probe");
        assert_eq!(mainline_call_ladder_labels(&function, &output), 3);

        output.anonymous_label_bump = 6;
        assert_eq!(mainline_call_ladder_labels(&function, &output), 1);

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
            attribute_alignment: None,
            row_bytes: None,
        });
        assert_eq!(mainline_call_ladder_labels(&function, &output), 0);
    }

    #[test]
    fn mainline_accounts_static_aggregate_initializer_nodes() {
        let mut function = function();
        function.locals.push(mwcc_syntax_trees::LocalDeclaration {
            declared_type: Type::Struct { size: 20, align: 4 },
            name: "tag".to_owned(),
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: true,
            data_bytes: Some(vec![0; 20]),
            data_relocations: vec![
                LocalDataRelocation {
                    offset: 0,
                    target: LocalDataRelocationTarget::StringLiteral(b"tag".to_vec()),
                    addend: 0,
                },
                LocalDataRelocation {
                    offset: 8,
                    target: LocalDataRelocationTarget::Symbol("callback".to_owned()),
                    addend: 0,
                },
            ],
            is_const: false,
            attribute_alignment: None,
            row_bytes: None,
        });
        function
            .statements
            .push(Statement::Expression(Expression::Call {
                name: "register".to_owned(),
                arguments: vec![Expression::AddressOf {
                    operand: Box::new(Expression::Variable("tag".to_owned())),
                }],
            }));

        assert_eq!(
            mainline_static_aggregate_initializer_labels(&function),
            7
        );
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

    #[test]
    fn transfers_a_scoped_bump_to_the_next_new_constant() {
        let pool = |bits| mwcc_machine_code::PoolConstant {
            bits,
            byte_width: 4,
            static_slot: false,
            image: false,
            force_new: false,
        };
        let mut machines = vec![
            MachineFunction::new("owner"),
            MachineFunction::new("reuse_only"),
            MachineFunction::new("new_pool"),
        ];
        machines[0].constants.push(pool(1));
        machines[0].deferred_next_constant_scope_bump = 24;
        machines[1].constants.push(pool(1));
        machines[2].constants.push(pool(2));

        apply_deferred_constant_scopes(&mut machines);

        assert_eq!(machines[1].constant_number_adjust, 0);
        assert_eq!(machines[1].post_function_counter_rollback, 0);
        assert_eq!(machines[2].constant_number_adjust, 24);
        assert_eq!(machines[2].post_function_counter_rollback, 24);
    }
}
