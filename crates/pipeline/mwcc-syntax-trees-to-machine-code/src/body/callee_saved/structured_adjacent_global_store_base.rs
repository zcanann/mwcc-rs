//! Branch-scoped address reuse for adjacent stores to one global aggregate.
//!
//! A contiguous run of direct member stores in one basic block shares a single
//! address materialization even when the function-wide aggregate cache
//! threshold is not met. The scope ends after the run, so unrelated arms and
//! later joins retain their own address live ranges.

use mwcc_syntax_trees::{Expression, Statement, Type};

pub(super) struct Plan {
    pub(super) global: String,
    pub(super) total_size: u32,
    pub(super) use_count: usize,
}

pub(super) fn plan(
    statements: &[Statement],
    addressable_globals: &std::collections::HashMap<String, Type>,
) -> Option<Plan> {
    let (first_global, first_value) = direct_global_member_store(statements.first()?)?;
    if crate::analysis::expression_has_call(first_value) {
        return None;
    }
    let use_count = statements
        .iter()
        .skip(1)
        .map_while(direct_global_member_store)
        .take_while(|(global, value)| {
            *global == first_global && !crate::analysis::expression_has_call(value)
        })
        .count()
        + 1;
    if use_count < 2 {
        return None;
    }
    let Type::Struct { size, .. } = addressable_globals.get(first_global).copied()? else {
        return None;
    };
    Some(Plan {
        global: first_global.to_owned(),
        total_size: u32::from(size),
        use_count,
    })
}

fn direct_global_member_store(statement: &Statement) -> Option<(&str, &Expression)> {
    let Statement::Store {
        target:
            Expression::Member {
                base,
                index_stride: None,
                ..
            },
        value,
    } = statement
    else {
        return None;
    };
    let Expression::Variable(global) = base.as_ref() else {
        return None;
    };
    Some((global, value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(global: &str, offset: u32) -> Statement {
        Statement::Store {
            target: Expression::Member {
                base: Box::new(Expression::Variable(global.into())),
                offset,
                member_type: Type::UnsignedInt,
                index_stride: None,
            },
            value: Expression::IntegerLiteral(0),
        }
    }

    #[test]
    fn plans_only_adjacent_stores_to_the_same_global() {
        let globals = std::collections::HashMap::from([(
            "state".into(),
            Type::Struct {
                size: 12,
                align: 4,
            },
        )]);
        let shared = plan(
            &[store("state", 4), store("state", 8), store("state", 12)],
            &globals,
        )
        .expect("shared base");
        assert_eq!(shared.global, "state");
        assert_eq!(shared.total_size, 12);
        assert_eq!(shared.use_count, 3);
        assert!(plan(&[store("state", 4), store("other", 8)], &globals).is_none());
    }
}
