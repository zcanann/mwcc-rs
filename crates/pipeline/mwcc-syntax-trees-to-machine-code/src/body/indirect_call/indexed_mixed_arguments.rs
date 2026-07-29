//! Indexed callback-table calls with mixed integer and floating arguments.
//!
//! The table lookup must precede argument marshaling because its address
//! calculation uses volatile integer scratch registers. The staged callback in
//! r12 then survives placement of the pointer argument in r3 and float value in
//! f1.

use super::*;

struct Shape<'a> {
    table: &'a str,
    index: &'a Expression,
    general_argument: &'a Expression,
    float_argument: &'a str,
}

impl Generator {
    pub(super) fn try_emit_indexed_indirect_call_with_mixed_arguments(
        &mut self,
        target: &Expression,
        arguments: &[Expression],
    ) -> Compilation<bool> {
        let Some(shape) = recognize(target, arguments) else {
            return Ok(false);
        };
        if !self.globals.contains_key(shape.table)
            || self.leaf_info(shape.index).is_err()
            || self.float_register_of(shape.float_argument).is_err()
        {
            return Ok(false);
        }

        self.evaluate(target, Type::UnsignedInt, 12)?;
        self.evaluate_float(
            &Expression::Variable(shape.float_argument.to_owned()),
            Eabi::FIRST_FLOAT_ARGUMENT,
        )?;
        self.evaluate_general(shape.general_argument, Eabi::FIRST_GENERAL_ARGUMENT)?;
        self.emit_indirect_branch_and_link(12);
        Ok(true)
    }
}

fn recognize<'a>(target: &'a Expression, arguments: &'a [Expression]) -> Option<Shape<'a>> {
    let Expression::Index { base, index } = target else {
        return None;
    };
    let Expression::Variable(table) = base.as_ref() else {
        return None;
    };
    let [general_argument, Expression::Variable(float_argument)] = arguments else {
        return None;
    };
    matches!(
        general_argument,
        Expression::Variable(_) | Expression::Member { .. } | Expression::Dereference { .. }
    )
    .then_some(Shape {
        table,
        index,
        general_argument,
        float_argument,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> Expression {
        Expression::Index {
            base: Box::new(Expression::Variable("callbacks".into())),
            index: Box::new(Expression::Variable("kind".into())),
        }
    }

    #[test]
    fn recognizes_a_pointer_and_float_argument_pair() {
        let arguments = vec![
            Expression::Member {
                base: Box::new(Expression::Variable("state".into())),
                offset: 4,
                member_type: Type::Pointer(Pointee::Int),
                index_stride: None,
            },
            Expression::Variable("value".into()),
        ];

        let callback = target();
        let shape = recognize(&callback, &arguments).expect("mixed callback shape");
        assert_eq!(shape.table, "callbacks");
        assert_eq!(shape.float_argument, "value");
    }

    #[test]
    fn rejects_a_computed_float_argument() {
        let arguments = vec![
            Expression::Variable("object".into()),
            Expression::Binary {
                operator: BinaryOperator::Add,
                left: Box::new(Expression::Variable("value".into())),
                right: Box::new(Expression::FloatLiteral(1.0)),
            },
        ];

        assert!(recognize(&target(), &arguments).is_none());
    }
}
