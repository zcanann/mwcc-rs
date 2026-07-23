//! SSA-style sinking for pure parameter reassignments in structured frames.
//!
//! MWCC keeps an incoming value in its saved home and materializes a pure
//! single-use rewrite directly into the eventual call-argument register. The
//! syntax tree records the source assignment earlier, so canonicalize that one
//! def-use edge before liveness and scheduling instead of teaching the emitter
//! to maintain a second mutable value environment.

use super::structured_locals::body_uses_local;
#[allow(unused_imports)]
use super::*;

pub(super) struct DenseEagerPointerRoundUp {
    pub(super) base_name: String,
    pub(super) pointer_name: String,
    pub(super) statement_index: usize,
    pub(super) rounded_expression: Expression,
}

pub(super) fn plan_dense_eager_pointer_round_up(
    function: &Function,
) -> Option<DenseEagerPointerRoundUp> {
    for local in &function.locals {
        if !is_byte_pointer(function, &local.name) {
            continue;
        }
        let Some(initializer) = local.initializer.as_ref() else {
            continue;
        };
        let Some((base, displacement)) = byte_pointer_displacement(initializer) else {
            continue;
        };
        let base_name = match base {
            Expression::Variable(name) => name,
            Expression::Cast { operand, .. } => match operand.as_ref() {
                Expression::Variable(name) => name,
                _ => continue,
            },
            _ => continue,
        };
        let Some((statement_index, rounded_expression)) = function
            .statements
            .iter()
            .enumerate()
            .find_map(|(index, statement)| match statement {
                Statement::Assign { name, value } if name == &local.name => {
                    compose_round_up_bias(value, &local.name, displacement)
                        .map(|expression| (index, expression))
                }
                _ => None,
            })
        else {
            continue;
        };
        return Some(DenseEagerPointerRoundUp {
            base_name: base_name.clone(),
            pointer_name: local.name.clone(),
            statement_index,
            rounded_expression,
        });
    }
    None
}

/// Compose a byte displacement immediately preceding a power-of-two pointer
/// round-up into the round-up's `addi` bias. MWCC keeps the intermediate pointer
/// in one register, but selects `load; addi combined_bias; clrrwi` rather than
/// materializing the source displacement separately.
pub(super) fn fold_adjacent_byte_pointer_round_up(function: &Function) -> Option<Function> {
    for index in 0..function.statements.len().saturating_sub(1) {
        let Statement::Assign {
            name,
            value: displaced,
        } = &function.statements[index]
        else {
            continue;
        };
        let Statement::Assign {
            name: rounded_name,
            value: rounded,
        } = &function.statements[index + 1]
        else {
            continue;
        };
        if name != rounded_name || !is_byte_pointer(function, name) {
            continue;
        }
        let Some((base, displacement)) = byte_pointer_displacement(displaced) else {
            continue;
        };
        let Some(composed_round_up) = compose_round_up_bias(rounded, name, displacement) else {
            continue;
        };

        let mut rewritten = function.clone();
        let Statement::Assign { value, .. } = &mut rewritten.statements[index] else {
            unreachable!("assignment was classified above")
        };
        *value = base.clone();
        let Statement::Assign { value, .. } = &mut rewritten.statements[index + 1] else {
            unreachable!("assignment was classified above")
        };
        *value = composed_round_up;
        return Some(rewritten);
    }
    None
}

/// Inline a single-use byte-pointer displacement into the immediately following
/// dereference. MWCC keeps the rounded base live and selects a displaced load;
/// retaining the source alias as an independent value instead produces an
/// unnecessary `addi` and consumes another register.
pub(super) fn fold_terminal_pointer_load_alias(function: &Function) -> Option<Function> {
    for index in 0..function.statements.len().saturating_sub(1) {
        let Statement::Assign {
            name,
            value: displaced,
        } = &function.statements[index]
        else {
            continue;
        };
        let Statement::Assign {
            name: destination,
            value: Expression::Dereference { pointer },
        } = &function.statements[index + 1]
        else {
            continue;
        };
        if destination == name
            || !is_byte_pointer(function, name)
            || byte_pointer_displacement(displaced).is_none()
            || count_name_occurrences(pointer, name) != 1
            || !pointer_is_cast_alias(pointer, name)
            || body_uses_local(&function.statements[index + 2..], name)
            || function
                .return_expression
                .as_ref()
                .is_some_and(|expression| expression_reads_name(expression, name))
        {
            continue;
        }

        let mut rewritten = function.clone();
        let Statement::Assign {
            value: Expression::Dereference { pointer },
            ..
        } = &mut rewritten.statements[index + 1]
        else {
            unreachable!("terminal load was classified above")
        };
        let Expression::Cast { operand, .. } = pointer.as_mut() else {
            unreachable!("pointer alias was classified above")
        };
        *operand = Box::new(displaced.clone());
        return Some(rewritten);
    }
    None
}

/// Whether normalization has made this pure alias assignment non-executable.
/// Keep the statement in the tree so legacy frame-residue accounting still sees
/// the source local, but do not materialize its now-unused register value.
pub(super) fn is_folded_terminal_pointer_load_alias(
    function: &Function,
    statement_index: usize,
) -> bool {
    let Some(Statement::Assign {
        name,
        value: displaced,
    }) = function.statements.get(statement_index)
    else {
        return false;
    };
    let Some(Statement::Assign {
        value: Expression::Dereference { pointer },
        ..
    }) = function.statements.get(statement_index + 1)
    else {
        return false;
    };
    is_byte_pointer(function, name)
        && byte_pointer_displacement(displaced).is_some()
        && !expression_reads_name(pointer, name)
        && !body_uses_local(&function.statements[statement_index + 1..], name)
}

fn pointer_is_cast_alias(expression: &Expression, name: &str) -> bool {
    matches!(
        expression,
        Expression::Cast {
            target_type: Type::Pointer(_) | Type::StructPointer { .. },
            operand,
        } if matches!(operand.as_ref(), Expression::Variable(alias) if alias == name)
    )
}

pub(super) fn adjacent_byte_pointer_round_up_name(function: &Function) -> Option<&str> {
    function
        .statements
        .windows(2)
        .find_map(|statements| match statements {
            [Statement::Assign {
                name,
                value: displaced,
            }, Statement::Assign {
                name: rounded_name,
                value: rounded,
            }] if name == rounded_name && is_byte_pointer(function, name) => {
                let foldable = byte_pointer_displacement(displaced)
                    .and_then(|(_, displacement)| {
                        compose_round_up_bias(rounded, name, displacement)
                    })
                    .is_some();
                (foldable || has_composed_round_up_bias(rounded, name)).then_some(name.as_str())
            }
            _ => None,
        })
}

/// A scaled member sum computed solely for the immediately following call is a
/// register-pressure value, not one of build 163's retained scalar frame lanes.
pub(super) fn is_transient_biased_scaled_member_call_local(
    statements: &[Statement],
    candidate: &str,
) -> bool {
    statements.windows(2).any(|window| {
        let [
            Statement::Assign { name, value },
            Statement::Expression(Expression::Call { arguments, .. }),
        ] = window
        else {
            return false;
        };
        name == candidate
            && arguments
                .iter()
                .any(|argument| matches!(argument, Expression::Variable(name) if name == candidate))
            && is_biased_scaled_member_sum(value)
    })
}

pub(super) fn is_transient_shifted_member_mask_call_local(
    statements: &[Statement],
    candidate: &str,
) -> bool {
    statements.windows(2).any(|window| {
        let [
            Statement::Assign { name, value },
            Statement::Expression(Expression::Call { arguments, .. }),
        ] = window
        else {
            return false;
        };
        name == candidate
            && arguments
                .iter()
                .any(|argument| matches!(argument, Expression::Variable(name) if name == candidate))
            && is_shifted_member_high_mask(value)
    })
}

/// A local whose reads are exclusively complete call arguments is a register
/// forwarding value. It does not consume one of the legacy frame's scalar-local
/// lanes even when its definition is kept as a separate syntax-tree statement.
pub(super) fn is_transient_direct_call_argument_local(
    statements: &[Statement],
    return_expression: Option<&Expression>,
    candidate: &str,
) -> bool {
    fn counts(statements: &[Statement], candidate: &str) -> (usize, usize, bool) {
        let mut total = 0;
        let mut direct = 0;
        let mut assigned = false;
        macro_rules! add_expression {
            ($expression:expr) => {{
                total += count_name_occurrences($expression, candidate);
                direct += count_direct_call_argument_occurrences($expression, candidate);
            }};
        }
        for statement in statements {
            match statement {
                Statement::Store { target, value } => {
                    add_expression!(target);
                    add_expression!(value);
                }
                Statement::Assign { name, value } => {
                    assigned |= name == candidate;
                    add_expression!(value);
                }
                Statement::Expression(expression) | Statement::Return(Some(expression)) => {
                    add_expression!(expression);
                }
                Statement::If {
                    condition,
                    then_body,
                    else_body,
                } => {
                    add_expression!(condition);
                    let then_counts = counts(then_body, candidate);
                    let else_counts = counts(else_body, candidate);
                    total += then_counts.0 + else_counts.0;
                    direct += then_counts.1 + else_counts.1;
                    assigned |= then_counts.2 || else_counts.2;
                }
                Statement::Loop {
                    initializer,
                    condition,
                    step,
                    body,
                    ..
                } => {
                    for expression in initializer.iter().chain(condition).chain(step) {
                        add_expression!(expression);
                    }
                    let body_counts = counts(body, candidate);
                    total += body_counts.0;
                    direct += body_counts.1;
                    assigned |= body_counts.2;
                }
                Statement::Switch { .. } => return (usize::MAX, 0, assigned),
                Statement::Return(None)
                | Statement::Break
                | Statement::Continue
                | Statement::Goto(_)
                | Statement::Label(_) => {}
            }
        }
        (total, direct, assigned)
    }

    let (mut total, mut direct, assigned) = counts(statements, candidate);
    if let Some(expression) = return_expression {
        total = total.saturating_add(count_name_occurrences(expression, candidate));
        direct = direct.saturating_add(count_direct_call_argument_occurrences(
            expression, candidate,
        ));
    }
    assigned && total != 0 && total == direct
}

fn is_shifted_member_high_mask(expression: &Expression) -> bool {
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left: combined,
        right: mask,
    } = expression
    else {
        return false;
    };
    let Some(mask) = constant_value(mask).map(|value| value as i32 as u32) else {
        return false;
    };
    let Expression::Binary {
        operator: BinaryOperator::BitXor,
        left: shifted,
        right: member,
    } = combined.as_ref()
    else {
        return false;
    };
    let Expression::Binary {
        operator: BinaryOperator::ShiftLeft,
        left: variable,
        right: shift,
    } = shifted.as_ref()
    else {
        return false;
    };
    let cleared_bits = mask.trailing_zeros();
    cleared_bits != 0
        && cleared_bits < 32
        && mask == u32::MAX << cleared_bits
        && matches!(variable.as_ref(), Expression::Variable(_))
        && constant_value(shift).is_some()
        && matches!(
            member.as_ref(),
            Expression::Member {
                index_stride: None,
                ..
            }
        )
}

fn is_biased_scaled_member_sum(expression: &Expression) -> bool {
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: product,
        right: tail,
    } = expression
    else {
        return false;
    };
    if constant_value(tail).is_none() {
        return false;
    }
    let Expression::Binary {
        operator: BinaryOperator::Multiply,
        left: sum,
        right: scale,
    } = product.as_ref()
    else {
        return false;
    };
    let Some(scale) = constant_value(scale).and_then(|value| u32::try_from(value).ok()) else {
        return false;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: biased,
        right: member,
    } = sum.as_ref()
    else {
        return false;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: variable,
        right: bias,
    } = biased.as_ref()
    else {
        return false;
    };
    scale >= 2
        && scale.is_power_of_two()
        && matches!(variable.as_ref(), Expression::Variable(_))
        && constant_value(bias).is_some()
        && matches!(
            member.as_ref(),
            Expression::Member {
                index_stride: None,
                ..
            }
        )
}

fn has_composed_round_up_bias(expression: &Expression, name: &str) -> bool {
    let Expression::Cast {
        target_type: Type::Pointer(_) | Type::StructPointer { .. },
        operand: masked,
    } = expression
    else {
        return false;
    };
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left,
        right,
    } = masked.as_ref()
    else {
        return false;
    };
    let (sum, mask) = if constant_value(right).is_some() {
        (left.as_ref(), right.as_ref())
    } else {
        (right.as_ref(), left.as_ref())
    };
    let Some(mask) = constant_value(mask).map(|value| value as i32 as u32) else {
        return false;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left,
        right,
    } = sum
    else {
        return false;
    };
    let (source, bias) = if let Some(bias) = constant_value(right) {
        (left.as_ref(), bias)
    } else if let Some(bias) = constant_value(left) {
        (right.as_ref(), bias)
    } else {
        return false;
    };
    let source_name = match source {
        Expression::Variable(source_name) => source_name.as_str(),
        Expression::Cast {
            target_type: Type::Int | Type::UnsignedInt,
            operand,
        } => match operand.as_ref() {
            Expression::Variable(source_name) => source_name.as_str(),
            _ => return false,
        },
        _ => return false,
    };
    let cleared_bits = mask.trailing_zeros();
    source_name == name
        && (1..=15).contains(&cleared_bits)
        && mask == u32::MAX << cleared_bits
        && bias > i64::from((1_u32 << cleared_bits) - 1)
}

fn is_byte_pointer(function: &Function, name: &str) -> bool {
    function
        .locals
        .iter()
        .find(|local| local.name == name)
        .map(|local| local.declared_type)
        .or_else(|| {
            function
                .parameters
                .iter()
                .find(|parameter| parameter.name == name)
                .map(|parameter| parameter.parameter_type)
        })
        .is_some_and(|ty| matches!(ty, Type::Pointer(Pointee::Char | Pointee::UnsignedChar)))
}

fn byte_pointer_displacement(expression: &Expression) -> Option<(&Expression, i64)> {
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left,
        right,
    } = expression
    else {
        return None;
    };
    let (base, displacement) = if let Some(displacement) = constant_value(right) {
        (left.as_ref(), displacement)
    } else {
        (right.as_ref(), constant_value(left)?)
    };
    (displacement > 0).then_some((base, displacement))
}

fn compose_round_up_bias(
    expression: &Expression,
    name: &str,
    displacement: i64,
) -> Option<Expression> {
    let Expression::Cast {
        target_type: target_type @ (Type::Pointer(_) | Type::StructPointer { .. }),
        operand: masked,
    } = expression
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left,
        right,
    } = masked.as_ref()
    else {
        return None;
    };
    let (sum, mask) = if constant_value(right).is_some() {
        (left.as_ref(), right.as_ref())
    } else {
        (right.as_ref(), left.as_ref())
    };
    let mask_value = constant_value(mask)? as i32 as u32;
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left,
        right,
    } = sum
    else {
        return None;
    };
    let (source, bias) = if let Some(bias) = constant_value(right) {
        (left.as_ref(), bias)
    } else {
        (right.as_ref(), constant_value(left)?)
    };
    let source_name = match source {
        Expression::Variable(source_name) => source_name,
        Expression::Cast {
            target_type: Type::Int | Type::UnsignedInt,
            operand,
        } => match operand.as_ref() {
            Expression::Variable(source_name) => source_name,
            _ => return None,
        },
        _ => return None,
    };
    let alignment = bias.checked_add(1)?;
    if source_name != name
        || !(2..=32768).contains(&alignment)
        || !u32::try_from(alignment).ok()?.is_power_of_two()
        || mask_value != !(u32::try_from(alignment).ok()? - 1)
    {
        return None;
    }
    let composed_bias = bias.checked_add(displacement)?;
    i16::try_from(composed_bias).ok()?;

    Some(Expression::Cast {
        target_type: *target_type,
        operand: Box::new(Expression::Binary {
            operator: BinaryOperator::BitAnd,
            left: Box::new(Expression::Binary {
                operator: BinaryOperator::Add,
                left: Box::new(source.clone()),
                right: Box::new(Expression::IntegerLiteral(composed_bias)),
            }),
            right: Box::new(mask.clone()),
        }),
    })
}

/// Sink a low-bit-clearing parameter assignment through uses which cannot
/// observe those bits. High right-shifts keep reading the original saved value;
/// a raw call argument receives the mask at that use. This is the SSA form MWCC
/// chooses on newer allocators for `x &= 0xfffff000` followed by command-byte
/// extraction and, optionally, one final raw argument.
pub(super) fn sink_low_mask_parameter_assignment(function: &Function) -> Option<Function> {
    for (assignment_index, statement) in function.statements.iter().enumerate() {
        let Statement::Assign {
            name,
            value:
                Expression::Binary {
                    operator: BinaryOperator::BitAnd,
                    left,
                    right,
                },
        } = statement
        else {
            continue;
        };
        if !function
            .parameters
            .iter()
            .any(|parameter| &parameter.name == name)
            || !matches!(left.as_ref(), Expression::Variable(read) if read == name)
        {
            continue;
        }
        let Some(mask_value) = constant_value(right) else {
            continue;
        };
        let mask = mask_value as u32;
        let cleared = mask.trailing_zeros();
        if cleared == 0 || mask != u32::MAX << cleared {
            continue;
        }

        let mut rewritten = function.clone();
        let mut supported = true;
        for later in rewritten.statements.iter_mut().skip(assignment_index + 1) {
            if !rewrite_low_mask_statement(later, name, mask_value, cleared) {
                supported = false;
                break;
            }
        }
        if !supported
            || rewritten
                .return_expression
                .as_ref()
                .is_some_and(|expression| expression_reads_name(expression, name))
        {
            continue;
        }
        rewritten.statements.remove(assignment_index);
        return Some(rewritten);
    }
    None
}

fn rewrite_low_mask_statement(
    statement: &mut Statement,
    name: &str,
    mask: i64,
    cleared: u32,
) -> bool {
    match statement {
        Statement::Store { target, value } => {
            !expression_reads_name(target, name) && high_shift_only(value, name, cleared)
        }
        Statement::Expression(Expression::Call { arguments, .. }) => {
            rewrite_low_mask_call_arguments(arguments, name, mask, cleared)
        }
        Statement::Assign {
            name: assigned,
            value,
        } => assigned != name && high_shift_only(value, name, cleared),
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            !expression_reads_name(condition, name)
                && then_body
                    .iter_mut()
                    .all(|statement| rewrite_low_mask_statement(statement, name, mask, cleared))
                && else_body
                    .iter_mut()
                    .all(|statement| rewrite_low_mask_statement(statement, name, mask, cleared))
        }
        Statement::Return(Some(value)) => high_shift_only(value, name, cleared),
        Statement::Return(None)
        | Statement::Goto(_)
        | Statement::Label(_)
        | Statement::Break
        | Statement::Continue => true,
        Statement::Switch { .. } | Statement::Loop { .. } => false,
        Statement::Expression(value) => !expression_reads_name(value, name),
    }
}

fn rewrite_low_mask_call_arguments(
    arguments: &mut [Expression],
    name: &str,
    mask: i64,
    cleared: u32,
) -> bool {
    for argument in arguments {
        if matches!(argument, Expression::Variable(read) if read == name) {
            *argument = Expression::Binary {
                operator: BinaryOperator::BitAnd,
                left: Box::new(Expression::Variable(name.to_string())),
                right: Box::new(Expression::IntegerLiteral(mask)),
            };
        } else if !high_shift_only(argument, name, cleared) {
            return false;
        }
    }
    true
}

fn high_shift_only(expression: &Expression, name: &str, cleared: u32) -> bool {
    if !expression_reads_name(expression, name) {
        return true;
    }
    match expression {
        Expression::Binary {
            operator: BinaryOperator::ShiftRight,
            left,
            right,
        } if matches!(left.as_ref(), Expression::Variable(read) if read == name) => {
            constant_value(right).is_some_and(|shift| shift >= i64::from(cleared))
        }
        Expression::Binary { left, right, .. } => {
            high_shift_only(left, name, cleared) && high_shift_only(right, name, cleared)
        }
        Expression::Unary { operand, .. } | Expression::Cast { operand, .. } => {
            high_shift_only(operand, name, cleared)
        }
        _ => false,
    }
}

pub(super) fn sink_single_use_parameter_assignment(function: &Function) -> Option<Function> {
    for (assignment_index, statement) in function.statements.iter().enumerate() {
        let Statement::Assign { name, value } = statement else {
            continue;
        };
        if !function
            .parameters
            .iter()
            .any(|parameter| &parameter.name == name)
            || !is_pure_parameter_rewrite(value, name)
            || function
                .return_expression
                .as_ref()
                .is_some_and(|expression| expression_reads_name(expression, name))
        {
            continue;
        }

        let mut use_site = None;
        let mut rejected = false;
        for (later_index, later) in function
            .statements
            .iter()
            .enumerate()
            .skip(assignment_index + 1)
        {
            if !statement_reads_name(later, name) {
                continue;
            }
            let Statement::Expression(Expression::Call { arguments, .. }) = later else {
                rejected = true;
                break;
            };
            let matching: Vec<usize> = arguments
                .iter()
                .enumerate()
                .filter_map(|(argument_index, argument)| {
                    matches!(argument, Expression::Variable(argument) if argument == name)
                        .then_some(argument_index)
                })
                .collect();
            let [argument_index] = matching.as_slice() else {
                rejected = true;
                break;
            };
            if use_site.replace((later_index, *argument_index)).is_some() {
                rejected = true;
                break;
            }
        }
        if rejected {
            continue;
        }
        let Some((call_index, argument_index)) = use_site else {
            continue;
        };

        let mut rewritten = function.clone();
        let Statement::Expression(Expression::Call { arguments, .. }) =
            &mut rewritten.statements[call_index]
        else {
            unreachable!("use site was classified as a call")
        };
        arguments[argument_index] = value.clone();
        rewritten.statements.remove(assignment_index);
        return Some(rewritten);
    }
    None
}

fn statement_reads_name(statement: &Statement, name: &str) -> bool {
    match statement {
        Statement::Store { target, value } => {
            expression_reads_name(target, name) || expression_reads_name(value, name)
        }
        Statement::Assign { value, .. }
        | Statement::Expression(value)
        | Statement::Return(Some(value)) => expression_reads_name(value, name),
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            expression_reads_name(condition, name)
                || then_body
                    .iter()
                    .any(|statement| statement_reads_name(statement, name))
                || else_body
                    .iter()
                    .any(|statement| statement_reads_name(statement, name))
        }
        Statement::Switch { .. } | Statement::Loop { .. } => true,
        Statement::Return(None)
        | Statement::Break
        | Statement::Continue
        | Statement::Goto(_)
        | Statement::Label(_) => false,
    }
}

fn is_pure_parameter_rewrite(expression: &Expression, name: &str) -> bool {
    match expression {
        Expression::Variable(variable) => variable == name,
        Expression::IntegerLiteral(_) => true,
        Expression::Unary { operand, .. } | Expression::Cast { operand, .. } => {
            is_pure_parameter_rewrite(operand, name)
        }
        Expression::Binary { left, right, .. } => {
            is_pure_parameter_rewrite(left, name)
                && is_pure_parameter_rewrite(right, name)
                && expression_reads_name(expression, name)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::Parameter;

    fn byte_pointer_local(name: &str, initializer: Expression) -> LocalDeclaration {
        LocalDeclaration {
            declared_type: Type::Pointer(Pointee::UnsignedChar),
            name: name.into(),
            initializer: Some(initializer),
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            row_bytes: None,
        }
    }

    #[test]
    fn plans_a_round_up_across_a_pointer_initializer_and_assignment() {
        let function = Function {
            return_type: Type::Void,
            name: "compiled".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![Parameter {
                parameter_type: Type::Pointer(Pointee::UnsignedChar),
                name: "base".into(),
            }],
            locals: vec![byte_pointer_local(
                "aligned",
                Expression::Binary {
                    operator: BinaryOperator::Add,
                    left: Box::new(Expression::Variable("base".into())),
                    right: Box::new(Expression::IntegerLiteral(16)),
                },
            )],
            statements: vec![Statement::Assign {
                name: "aligned".into(),
                value: Expression::Cast {
                    target_type: Type::Pointer(Pointee::UnsignedChar),
                    operand: Box::new(Expression::Binary {
                        operator: BinaryOperator::BitAnd,
                        left: Box::new(Expression::Binary {
                            operator: BinaryOperator::Add,
                            left: Box::new(Expression::Cast {
                                target_type: Type::UnsignedInt,
                                operand: Box::new(Expression::Variable("aligned".into())),
                            }),
                            right: Box::new(Expression::IntegerLiteral(31)),
                        }),
                        right: Box::new(Expression::IntegerLiteral(!31_i64)),
                    }),
                },
            }],
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };

        let plan = plan_dense_eager_pointer_round_up(&function).unwrap();
        assert_eq!(plan.base_name, "base");
        assert_eq!(plan.pointer_name, "aligned");
        assert_eq!(plan.statement_index, 0);
        assert!(has_composed_round_up_bias(
            &plan.rounded_expression,
            "aligned"
        ));
    }

    #[test]
    fn distinguishes_call_forwarding_from_an_ordinary_local_read() {
        let statements = vec![
            Statement::Assign {
                name: "length".into(),
                value: Expression::IntegerLiteral(20),
            },
            Statement::If {
                condition: Expression::Binary {
                    operator: BinaryOperator::Less,
                    left: Box::new(Expression::Call {
                        name: "read".into(),
                        arguments: vec![Expression::Variable("length".into())],
                    }),
                    right: Box::new(Expression::IntegerLiteral(0)),
                },
                then_body: Vec::new(),
                else_body: Vec::new(),
            },
        ];

        assert!(is_transient_direct_call_argument_local(
            &statements,
            None,
            "length"
        ));
        assert!(!is_transient_direct_call_argument_local(
            &statements,
            Some(&Expression::Variable("length".into())),
            "length"
        ));
    }
}
