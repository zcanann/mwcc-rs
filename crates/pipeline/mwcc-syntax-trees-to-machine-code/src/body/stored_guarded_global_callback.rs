//! Member stores followed by an inlined guarded callback-table dispatch.
//!
//! Inline expansion leaves a wrapper alias for the stores and a separately
//! reloaded helper alias for dispatch. This owner schedules that whole
//! transaction without adding wrapper-specific state to the direct callback
//! owner.

#[allow(unused_imports)]
use super::*;
use super::guarded_global_callback::{
    callback_statement, either_null_callback, member_offset, variable,
};

struct PrefixStore<'a> {
    offset: i16,
    value: &'a str,
}

struct Shape<'a> {
    object: &'a str,
    stores: Vec<PrefixStore<'a>>,
    store_alias_offset: i16,
    callback_alias_offset: i16,
    first_guard_offset: i16,
    second_guard_offset: i16,
    selector_offset: i16,
    callback_table: &'a str,
}

fn general_parameter(parameter_type: Type) -> bool {
    matches!(
        parameter_type,
        Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. }
    )
}

fn classify(function: &Function) -> Option<Shape<'_>> {
    if function.return_type != Type::Void
        || function.return_expression.is_some()
        || !function.guards.is_empty()
        || function.parameters.len() < 2
        || !function
            .parameters
            .iter()
            .skip(1)
            .all(|parameter| general_parameter(parameter.parameter_type))
    {
        return None;
    }
    let object = function.parameters.first()?;
    if !matches!(
        object.parameter_type,
        Type::Pointer(_) | Type::StructPointer { .. }
    ) {
        return None;
    }
    let [store_alias, callback_alias] = function.locals.as_slice() else {
        return None;
    };
    if !matches!(
        store_alias.declared_type,
        Type::Pointer(_) | Type::StructPointer { .. }
    ) || !matches!(
        callback_alias.declared_type,
        Type::Pointer(_) | Type::StructPointer { .. }
    ) {
        return None;
    }
    let store_alias_offset = member_offset(store_alias.initializer.as_ref()?, &object.name)?;
    let store_count = function.parameters.len() - 1;
    let (store_statements, tail) = function.statements.split_at_checked(store_count)?;
    let (callback_alias_offset, guarded_callback) = match tail {
        [Statement::Assign { name, value }, guarded] if name == &callback_alias.name => {
            (member_offset(value, &object.name)?, guarded)
        }
        [guarded] => (
            member_offset(callback_alias.initializer.as_ref()?, &object.name)?,
            guarded,
        ),
        _ => return None,
    };
    let stores = store_statements
        .iter()
        .zip(function.parameters.iter().skip(1))
        .map(|(statement, parameter)| {
            let Statement::Store { target, value } = statement else {
                return None;
            };
            if !variable(value, &parameter.name) {
                return None;
            }
            Some(PrefixStore {
                offset: member_offset(target, &store_alias.name)?,
                value: parameter.name.as_str(),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let (first_guard_offset, second_guard_offset, callback) =
        either_null_callback(guarded_callback, &callback_alias.name)?;
    let (callback_table, selector_offset) =
        callback_statement(callback, &callback_alias.name, &object.name, None)?;
    Some(Shape {
        object: &object.name,
        stores,
        store_alias_offset,
        callback_alias_offset,
        first_guard_offset,
        second_guard_offset,
        selector_offset,
        callback_table,
    })
}

impl Generator {
    pub(crate) fn try_stored_guarded_global_callback(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        if self.general_register_of(shape.object)? != 3
            || shape.stores.len() >= 8
            || shape.stores.iter().enumerate().any(|(index, store)| {
                self.general_register_of(store.value).ok() != u8::try_from(index + 4).ok()
            })
            || !self.globals.contains_key(shape.callback_table)
        {
            return Ok(false);
        }

        self.output.pre_scheduled = true;
        self.non_leaf = true;
        self.frame_size = 8;
        let done = self.fresh_label();
        self.output.instructions.extend([
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -8,
            },
        ]);
        let store_base = 4 + u8::try_from(shape.stores.len()).unwrap();
        self.output.instructions.push(Instruction::LoadWord {
            d: store_base,
            a: 3,
            offset: shape.store_alias_offset,
        });
        for (index, store) in shape.stores.iter().enumerate() {
            self.output.instructions.push(Instruction::StoreWord {
                s: 4 + u8::try_from(index).unwrap(),
                a: store_base,
                offset: store.offset,
            });
        }
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 3,
            offset: shape.callback_alias_offset,
        });
        self.emit_either_null_guarded_callback(
            4,
            shape.first_guard_offset,
            shape.second_guard_offset,
            shape.selector_offset,
            shape.callback_table,
            done,
        );
        self.bind_label(done);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 12,
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 8,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_general_parameters_can_feed_prefix_stores() {
        assert!(general_parameter(Type::Int));
        assert!(general_parameter(Type::Pointer(Pointee::Int)));
        assert!(!general_parameter(Type::Float));
    }
}
