//! mwcc's symbol-table ordering for a function's referenced names.
//!
//! mwcc assigns symbol-table indices to the globals and callees a function
//! references by an AST traversal — NOT by `.text` reference (offset) order. The
//! traversal is left-to-right except that a binary node visits its RIGHT operand
//! first when the right is a leaf and the left is compound (so `g*2 + h` registers
//! `h` before `g`). Mainline groups by KIND: every DATA reference first, then every
//! CALL target. Build 163 instead preserves creation order across kinds, visits an
//! assignment's value before its target, and registers data-only subtraction
//! operands right-first. The object writer consumes the resolved order uniformly.

use mwcc_syntax_trees::{
    AsmItem, AsmOperand, BinaryOperator, Expression, Function, Statement, Type,
};
use mwcc_machine_code::{Relocation, RelocationKind, RelocationTarget};
use mwcc_versions::SymbolTraversalStyle;

use crate::intrinsics::is_intrinsic_call;

/// The data and call references collected during the traversal, kept apart so the
/// data group can be emitted ahead of the call group.
struct Names<'a> {
    data: Vec<String>,
    calls: Vec<String>,
    creation_order: Vec<String>,
    /// Prototyped and defined function names. A function designator used as a
    /// value (`register_handler(..., callback)`) belongs to mwcc's function
    /// symbol group, not the data group, even though the AST leaf is a Variable.
    functions: &'a std::collections::HashMap<String, Type>,
    traversal: SymbolTraversalStyle,
}

impl Names<'_> {
    fn push_data(&mut self, name: String) {
        self.creation_order.push(name.clone());
        self.data.push(name);
    }

    fn push_call(&mut self, name: String) {
        self.creation_order.push(name.clone());
        self.calls.push(name);
    }
}

/// The referenced names (globals, callees, and locals — the writer filters to the
/// ones that become symbols) in mwcc's symbol-table order: all data references,
/// then all call targets, deduplicated to first occurrence.
pub(crate) fn referenced_names(
    function: &Function,
    functions: &std::collections::HashMap<String, Type>,
    traversal: SymbolTraversalStyle,
) -> Vec<String> {
    let mut names = Names {
        data: Vec::new(),
        calls: Vec::new(),
        creation_order: Vec::new(),
        functions,
        traversal,
    };
    // A local's initializer is evaluated first (in source order), so a call/global it
    // references is numbered ahead of the body's — `int z = g(); h();` lists g before h.
    for local in &function.locals {
        if let Some(initializer) = &local.initializer {
            collect(initializer, &mut names);
        }
    }
    for statement in &function.statements {
        collect_statement(statement, &mut names);
    }
    for guard in &function.guards {
        collect(&guard.condition, &mut names);
        collect(&guard.value, &mut names);
    }
    if let Some(expression) = &function.return_expression {
        collect(expression, &mut names);
    }
    let mut ordered = if traversal == SymbolTraversalStyle::LegacyCreationOrder {
        names.creation_order
    } else {
        let mut ordered = names.data;
        ordered.extend(names.calls);
        ordered
    };
    let mut seen = std::collections::HashSet::new();
    ordered.retain(|name| seen.insert(name.clone()));
    ordered
}

/// Insert compiler-created call targets into the source-derived symbol stream.
///
/// Source traversal deliberately keeps data references ahead of calls and can
/// differ from final instruction order. A runtime helper has no source node,
/// however, so MWCC creates its symbol while lowering the conversion. Compare
/// it only with other direct-call references: this preserves the source data
/// group while placing the hidden call before the next call lowered after it.
pub(crate) fn interleave_compiler_generated_calls(
    ordered: &mut Vec<String>,
    generated: &[String],
    relocations: &[Relocation],
) {
    let first_direct_call = |wanted: &str| {
        relocations
            .iter()
            .filter(|relocation| relocation.kind == RelocationKind::Rel24)
            .filter_map(|relocation| match &relocation.target {
                RelocationTarget::External(name) if name == wanted => {
                    Some(relocation.instruction_index)
                }
                _ => None,
            })
            .min()
    };
    let mut events: Vec<_> = generated
        .iter()
        .filter_map(|name| first_direct_call(name).map(|index| (index, name)))
        .collect();
    events.sort_by_key(|(index, _)| *index);
    for (event_index, name) in events {
        if ordered.iter().any(|listed| listed == name) {
            continue;
        }
        let insertion = ordered
            .iter()
            .position(|listed| {
                first_direct_call(listed).is_some_and(|index| index > event_index)
            })
            .unwrap_or(ordered.len());
        ordered.insert(insertion, name.clone());
    }
}

/// A leaf operand — a name or a literal — needs no computation; everything else is
/// compound. The binary visit order keys off this.
fn is_leaf(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Variable(_) | Expression::IntegerLiteral(_) | Expression::FloatLiteral(_)
    )
}

fn contains_call(expression: &Expression) -> bool {
    match expression {
        Expression::Call { name, arguments } if is_intrinsic_call(name, arguments.len()) => {
            arguments.iter().any(contains_call)
        }
        Expression::Call { .. }
        | Expression::CallThrough { .. }
        | Expression::VirtualCall { .. }
        | Expression::ConstructedNew { .. } => true,
        Expression::Binary { left, right, .. }
        | Expression::Comma { left, right }
        | Expression::Assign {
            target: left,
            value: right,
        } => contains_call(left) || contains_call(right),
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => contains_call(condition) || contains_call(when_true) || contains_call(when_false),
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::BitFieldRead {
            extracted: operand, ..
        }
        | Expression::IndexedUpdateValue { value: operand }
        | Expression::AddressOf { operand }
        | Expression::Dereference { pointer: operand }
        | Expression::PostStep {
            target: operand, ..
        } => contains_call(operand),
        Expression::Index { base, index } => contains_call(base) || contains_call(index),
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => {
            contains_call(base)
        }
        Expression::Variable(_)
        | Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::CompoundLiteral { .. }
        | Expression::AggregateLiteral(_) => false,
    }
}

/// `target = value`, visited as a binary node.
fn collect_assignment(target: &Expression, value: &Expression, names: &mut Names) {
    if names.traversal == SymbolTraversalStyle::LegacyCreationOrder {
        collect(value, names);
        collect(target, names);
        return;
    }
    // A comma-operator value evaluates its left for side effects before the store, so the
    // left's symbols register ahead of the target (`gi = (gh=a, b)` registers gh then gi).
    if let Expression::Comma { left, right } = value {
        collect(left, names);
        collect_assignment(target, right, names);
        return;
    }
    if is_leaf(value) && !is_leaf(target) {
        collect(value, names);
        collect(target, names);
    } else {
        collect(target, names);
        collect(value, names);
    }
}

fn collect_statement(statement: &Statement, names: &mut Names) {
    match statement {
        Statement::InlineAsm(items) => {
            for item in items {
                let AsmItem::Instruction(instruction) = item else {
                    continue;
                };
                for operand in &instruction.operands {
                    let name = match operand {
                        AsmOperand::Symbol { name, .. }
                        | AsmOperand::SymbolMemory { name, .. }
                        | AsmOperand::SmallDataSymbolMemory { name, .. } => Some(name),
                        AsmOperand::Label(name)
                            if matches!(instruction.mnemonic.as_str(), "b" | "bl") =>
                        {
                            Some(name)
                        }
                        _ => None,
                    };
                    if let Some(name) = name {
                        if matches!(instruction.mnemonic.as_str(), "b" | "bl") {
                            names.push_call(name.clone());
                        } else {
                            names.push_data(name.clone());
                        }
                    }
                }
            }
        }
        Statement::Break | Statement::Continue | Statement::Goto(_) | Statement::Label(_) => {}
        Statement::Store { target, value } => collect_assignment(target, value, names),
        Statement::Assign { value, .. } => collect(value, names),
        Statement::Expression(expression) => collect(expression, names),
        Statement::Switch {
            scrutinee,
            arms,
            default,
        } => {
            collect(scrutinee, names);
            for arm in arms {
                match &arm.body {
                    mwcc_syntax_trees::ArmBody::Return(result) => collect(result, names),
                    mwcc_syntax_trees::ArmBody::Statements(statements) => {
                        for statement in statements {
                            collect_statement(statement, names);
                        }
                    }
                }
            }
            match default {
                Some(mwcc_syntax_trees::ArmBody::Return(result)) => collect(result, names),
                Some(mwcc_syntax_trees::ArmBody::Statements(statements)) => {
                    for statement in statements {
                        collect_statement(statement, names);
                    }
                }
                None => {}
            }
        }
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            collect(condition, names);
            for statement in then_body.iter().chain(else_body) {
                collect_statement(statement, names);
            }
        }
        Statement::Loop {
            initializer,
            condition,
            step,
            body,
            ..
        } => {
            for expression in initializer.iter().chain(condition).chain(step) {
                collect(expression, names);
            }
            for statement in body {
                collect_statement(statement, names);
            }
        }
        Statement::Return(value) => {
            if let Some(value) = value {
                collect(value, names);
            }
        }
    }
}

fn collect(expression: &Expression, names: &mut Names) {
    match expression {
        Expression::CompoundLiteral { .. } => {}
        Expression::CallThrough { target, arguments } => {
            collect(target, names);
            for argument in arguments {
                collect(argument, names);
            }
        }
        Expression::VirtualCall {
            object, arguments, ..
        } => {
            collect(object, names);
            for argument in arguments {
                collect(argument, names);
            }
        }
        Expression::AggregateLiteral(_) => {}
        Expression::PostStep { target, .. } => collect(target, names),
        Expression::Variable(name) => {
            if names.functions.contains_key(name) {
                names.push_call(name.clone());
            } else {
                names.push_data(name.clone());
            }
        }
        Expression::Comma { left, right } => {
            collect(left, names);
            collect(right, names);
        }
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_) => {}
        Expression::Binary {
            operator,
            left,
            right,
        } => {
            // Short-circuit operands become distinct CFG regions and are
            // registered in evaluation order.  The ordinary expression-tree
            // leaf-hoisting rule does not cross that control-flow boundary.
            if *operator == BinaryOperator::Subtract
                && names.traversal == SymbolTraversalStyle::LegacyCreationOrder
                && !contains_call(left)
                && !contains_call(right)
            {
                collect(right, names);
                collect(left, names);
            } else if matches!(
                operator,
                BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr
            ) {
                collect(left, names);
                collect(right, names);
            } else if is_leaf(right) && !is_leaf(left) {
                collect(right, names);
                collect(left, names);
            } else {
                collect(left, names);
                collect(right, names);
            }
        }
        Expression::Unary { operand, .. } => collect(operand, names),
        Expression::Cast { operand, .. }
        | Expression::BitFieldRead {
            extracted: operand, ..
        }
        | Expression::IndexedUpdateValue { value: operand } => collect(operand, names),
        Expression::Dereference { pointer } => collect(pointer, names),
        Expression::AddressOf { operand } => collect(operand, names),
        Expression::Index { base, index } => {
            collect(base, names);
            collect(index, names);
        }
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => {
            collect(base, names)
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            collect(condition, names);
            collect(when_true, names);
            collect(when_false, names);
        }
        // A call registers its arguments (left to right) as data references, then
        // the callee into the call group (emitted after all data references).
        Expression::Call { name, arguments } => {
            if is_intrinsic_call(name, arguments.len()) {
                for argument in arguments {
                    collect(argument, names);
                }
                return;
            }
            if names.traversal == SymbolTraversalStyle::LegacyCreationOrder
                && arguments.iter().skip(1).any(contains_call)
            {
                for argument in arguments
                    .iter()
                    .skip(1)
                    .filter(|argument| contains_call(argument))
                {
                    collect(argument, names);
                }
                for argument in arguments.iter().filter(|argument| !contains_call(argument)) {
                    collect(argument, names);
                }
            } else {
                for argument in arguments {
                    collect(argument, names);
                }
            }
            names.push_call(name.clone());
        }
        Expression::ConstructedNew {
            allocation,
            constructor,
            arguments,
            ..
        } => {
            collect(allocation, names);
            for argument in arguments {
                collect(argument, names);
            }
            names.push_call(constructor.clone());
        }
        Expression::Assign { target, value } => collect_assignment(target, value, names),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relocation(index: usize, kind: RelocationKind, name: &str) -> Relocation {
        Relocation {
            instruction_index: index,
            kind,
            target: RelocationTarget::External(name.to_owned()),
        }
    }

    #[test]
    fn generated_call_is_interleaved_without_disturbing_the_data_group() {
        let mut ordered = vec!["global".into(), "before".into(), "after".into()];
        let generated = vec!["__runtime".into()];
        let relocations = vec![
            relocation(20, RelocationKind::EmbSda21, "global"),
            relocation(4, RelocationKind::Rel24, "before"),
            relocation(8, RelocationKind::Rel24, "__runtime"),
            relocation(12, RelocationKind::Rel24, "after"),
        ];

        interleave_compiler_generated_calls(&mut ordered, &generated, &relocations);

        assert_eq!(ordered, ["global", "before", "__runtime", "after"]);
    }
}
