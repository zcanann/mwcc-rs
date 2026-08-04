//! Recognition of functions whose long strings amortize one `.data` base.
//!
//! Writable long string literals share the translation unit's `...data.0`
//! section. Linkage-first MWCC retains that base once a function uses at least
//! three distinct long strings, allowing every later address to be one `addi`.

use std::collections::HashSet;

use mwcc_syntax_trees::{Expression, Function};

use super::structured_expression_visit::{visit_expression, visit_statement};

pub(super) fn owns_long_string_data_anchor(function: &Function) -> bool {
    let mut strings = HashSet::new();
    let mut collect = |expression: &Expression| {
        if let Expression::StringLiteral(bytes) = expression {
            if bytes.len() + 1 > 8 {
                strings.insert(bytes.clone());
            }
        }
    };
    for statement in &function.statements {
        visit_statement(statement, &mut collect);
    }
    if let Some(expression) = &function.return_expression {
        visit_expression(expression, &mut collect);
    }
    strings.len() >= 3
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{Statement, Type};

    fn function(strings: &[&[u8]]) -> Function {
        Function {
            return_type: Type::Void,
            name: "formats".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: Vec::new(),
            statements: strings
                .iter()
                .map(|bytes| Statement::Expression(Expression::StringLiteral(bytes.to_vec())))
                .collect(),
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
    fn accepts_three_distinct_long_strings() {
        assert!(owns_long_string_data_anchor(&function(&[
            b"first long format",
            b"second long format",
            b"third long format",
        ])));
    }

    #[test]
    fn ignores_short_and_duplicate_strings() {
        assert!(!owns_long_string_data_anchor(&function(&[
            b"same long format",
            b"same long format",
            b"short",
        ])));
    }
}
