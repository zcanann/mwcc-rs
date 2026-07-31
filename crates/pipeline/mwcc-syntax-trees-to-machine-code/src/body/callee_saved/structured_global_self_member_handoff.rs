//! Adjacent write/read handoff for a global pointer advanced through itself.
//!
//! In `current = current->next; consume(current->prev)`, the assigned pointer
//! is both the global store value and the base of the immediately following
//! member argument. MWCC keeps that value in r3 across the store. This module
//! owns the source-level adjacency proof; ordinary stores remain free to use
//! the scratch register.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn try_emit_structured_global_self_member_handoff(
        &mut self,
        statement: &Statement,
        next: Option<&Statement>,
    ) -> Compilation<bool> {
        if self.behavior.stored_global_read_style
            == mwcc_versions::StoredGlobalReadStyle::ReloadAfterStore
        {
            return Ok(false);
        }
        let Some(next) = next else {
            return Ok(false);
        };
        let Some((global, value, pointee)) =
            recognize(statement, next, &self.globals, &self.volatile_globals)
        else {
            return Ok(false);
        };
        let global = global.to_owned();
        let value = value.clone();
        self.evaluate_general(&value, Eabi::FIRST_GENERAL_ARGUMENT)?;
        self.emit_global_store(&global, pointee, Eabi::FIRST_GENERAL_ARGUMENT)?;
        self.stored_globals.insert(
            global,
            (Eabi::FIRST_GENERAL_ARGUMENT, self.output.instructions.len()),
        );
        Ok(true)
    }
}

fn recognize<'a>(
    statement: &'a Statement,
    next: &'a Statement,
    globals: &std::collections::HashMap<String, Type>,
    volatile_globals: &std::collections::HashSet<String>,
) -> Option<(&'a str, &'a Expression, Pointee)> {
    let Statement::Store {
        target: Expression::Variable(target),
        value:
            value @ Expression::Member {
                base,
                member_type: Type::Pointer(_) | Type::StructPointer { .. },
                index_stride: None,
                ..
            },
    } = statement
    else {
        return None;
    };
    if volatile_globals.contains(target)
        || !matches!(base.as_ref(), Expression::Variable(name) if name == target)
    {
        return None;
    }
    let global_type = *globals.get(target)?;
    if !matches!(global_type, Type::Pointer(_) | Type::StructPointer { .. }) {
        return None;
    }
    let Statement::Expression(Expression::Call { arguments, .. }) = next else {
        return None;
    };
    if !arguments
        .iter()
        .any(|argument| is_member_of_global(argument, target))
    {
        return None;
    }
    Some((target, value, pointee_of_type(global_type)?))
}

fn is_member_of_global(expression: &Expression, global: &str) -> bool {
    matches!(
        expression,
        Expression::Member {
            base,
            index_stride: None,
            ..
        } if matches!(base.as_ref(), Expression::Variable(name) if name == global)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(offset: u32) -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable("current".into())),
            offset,
            member_type: Type::StructPointer { element_size: 64 },
            index_stride: None,
        }
    }

    #[test]
    fn recognizes_an_adjacent_self_member_store_and_member_call() {
        let store = Statement::Store {
            target: Expression::Variable("current".into()),
            value: member(56),
        };
        let call = Statement::Expression(Expression::Call {
            name: "consume".into(),
            arguments: vec![member(60)],
        });
        let globals = std::collections::HashMap::from([(
            "current".into(),
            Type::StructPointer { element_size: 64 },
        )]);

        assert!(recognize(&store, &call, &globals, &std::collections::HashSet::new(),).is_some());
    }

    #[test]
    fn rejects_a_following_call_that_does_not_read_the_updated_global() {
        let store = Statement::Store {
            target: Expression::Variable("current".into()),
            value: member(56),
        };
        let call = Statement::Expression(Expression::Call {
            name: "consume".into(),
            arguments: vec![Expression::Variable("other".into())],
        });
        let globals = std::collections::HashMap::from([(
            "current".into(),
            Type::StructPointer { element_size: 64 },
        )]);

        assert!(recognize(&store, &call, &globals, &std::collections::HashSet::new(),).is_none());
    }
}
