//! Structured placement for inline-composed declaration initializers.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Evaluate a declaration initializer while preserving compiler-created
    /// setup expressions at the declaration site.
    ///
    /// Value-inline substitution declares hygienic temporaries separately but
    /// leaves their assignments in the original initializer as a comma prefix.
    /// Structured lowering already owns homes for those declarations; emit the
    /// prefix for its effects and place only the terminal value in the owning
    /// local's destination.
    pub(super) fn evaluate_structured_initializer(
        &mut self,
        function: &Function,
        initializer: &Expression,
        value_type: Type,
        destination: u8,
    ) -> Compilation<()> {
        if !has_inline_assignment_prefix(initializer) {
            return self.evaluate(initializer, value_type, destination);
        }
        let mut temporary_locations = Vec::new();
        for local in &function.locals {
            if !local.name.starts_with("__mwcc_inline_")
                || self.locations.contains_key(&local.name)
                || self.frame_slots.contains_key(&local.name)
                || (!crate::analysis::expression_reads_name(initializer, &local.name)
                    && super::structured_locals::expression_assignment_count(
                        initializer,
                        &local.name,
                    ) == 0)
            {
                continue;
            }
            let Ok(class) = class_of(local.declared_type) else {
                return Err(Diagnostic::error(
                    "an inline initializer temporary has an unsupported value class",
                ));
            };
            let register = match class {
                ValueClass::General => self.fresh_virtual_general(),
                ValueClass::Float => self.fresh_virtual_float(),
            };
            self.locations.insert(
                local.name.clone(),
                Location {
                    class,
                    register,
                    signed: self.signed_of(local.declared_type),
                    width: local.declared_type.width(),
                    pointee: match local.declared_type {
                        Type::Pointer(pointee) => Some(pointee),
                        _ => None,
                    },
                    stride: pointer_stride(local.declared_type),
                },
            );
            temporary_locations.push(local.name.clone());
        }
        let mut value = initializer;
        let result = (|| {
            while let Expression::Comma { left, right } = value {
                self.emit_comma_side_effect(left)?;
                value = right;
            }
            self.evaluate(value, value_type, destination)
        })();
        for name in temporary_locations {
            self.locations.remove(&name);
        }
        result
    }
}

fn has_inline_assignment_prefix(expression: &Expression) -> bool {
    let mut value = expression;
    let mut saw_prefix = false;
    while let Expression::Comma { left, right } = value {
        let Expression::Assign { target, .. } = left.as_ref() else {
            return false;
        };
        let Expression::Variable(name) = target.as_ref() else {
            return false;
        };
        if !name.starts_with("__mwcc_inline_") {
            return false;
        }
        saw_prefix = true;
        value = right;
    }
    saw_prefix
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_only_hygienic_inline_assignment_prefixes() {
        let expression = Expression::Comma {
            left: Box::new(Expression::Assign {
                target: Box::new(Expression::Variable("__mwcc_inline_helper_0_value".into())),
                value: Box::new(Expression::IntegerLiteral(7)),
            }),
            right: Box::new(Expression::Variable(
                "__mwcc_inline_helper_0_value".into(),
            )),
        };
        assert!(has_inline_assignment_prefix(&expression));

        let source_comma = Expression::Comma {
            left: Box::new(Expression::Assign {
                target: Box::new(Expression::Variable("source_local".into())),
                value: Box::new(Expression::IntegerLiteral(7)),
            }),
            right: Box::new(Expression::Variable("source_local".into())),
        };
        assert!(!has_inline_assignment_prefix(&source_comma));
    }
}
