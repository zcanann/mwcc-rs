//! Source-language projection of executable C++ ABI syntax trees.

use mwcc_syntax_trees::{BinaryOperator, Expression, Statement, TranslationUnit, Type};

/// Remove ABI-only state from otherwise empty deleting destructors before
/// generating source-level debug types and parameter DIEs.
pub(super) fn normalize(unit: &TranslationUnit) -> TranslationUnit {
    let mut source = unit.clone();
    for function in &mut source.functions {
        if !is_trivial_deleting_destructor(function) {
            continue;
        }
        function.return_type = Type::Void;
        function.parameters.pop();
        function.statements.clear();
        function.return_expression = None;
    }
    source
}

fn is_trivial_deleting_destructor(function: &mwcc_syntax_trees::Function) -> bool {
    if !function.name.starts_with("__dt__")
        || function.parameters.len() != 2
        || function.parameters[0].name != "this"
        || function.parameters[1].name != "__destroy"
        || function.parameters[1].parameter_type != Type::Short
        || !matches!(function.return_type, Type::StructPointer { .. })
        || !matches!(
            function.return_expression.as_ref(),
            Some(Expression::Variable(name)) if name == "this"
        )
    {
        return false;
    }
    let [Statement::If {
        condition: Expression::Variable(condition),
        then_body,
        else_body,
    }] = function.statements.as_slice()
    else {
        return false;
    };
    let [Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Greater,
                left,
                right,
            },
        then_body: delete_body,
        else_body: delete_else,
    }] = then_body.as_slice()
    else {
        return false;
    };
    condition == "this"
        && else_body.is_empty()
        && matches!(left.as_ref(), Expression::Variable(name) if name == "__destroy")
        && matches!(right.as_ref(), Expression::IntegerLiteral(0))
        && matches!(
            delete_body.as_slice(),
            [Statement::Expression(Expression::Call { arguments, .. })]
                if matches!(arguments.as_slice(), [Expression::Variable(name)] if name == "this")
        )
        && delete_else.is_empty()
}
