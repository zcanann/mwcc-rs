//! Leaf constructor-style member-store schedules.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Lower `p->a = value; p->b = C1; p->c = C2;` when `p` and `value` are
    /// incoming integer-class parameters. After the first store, mwcc reuses
    /// the dead value register for C1 and puts C2 in r0, materializing both
    /// constants before issuing their stores.
    pub(crate) fn try_member_parameter_two_constant_fill(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if function.return_type != Type::Void
            || function.return_expression.is_some()
            || !function.locals.is_empty()
            || !function.guards.is_empty()
            || function_makes_call(function)
        {
            return Ok(false);
        }
        let [first, second, third] = function.statements.as_slice() else {
            return Ok(false);
        };
        let Statement::Store {
            target:
                Expression::Member {
                    base: first_base,
                    offset: first_offset,
                    member_type: first_type,
                    index_stride: None,
                },
            value: Expression::Variable(value_name),
        } = first
        else {
            return Ok(false);
        };
        let Expression::Variable(base_name) = first_base.as_ref() else {
            return Ok(false);
        };
        if base_name == value_name {
            return Ok(false);
        }
        let Some(base_parameter) = function
            .parameters
            .iter()
            .find(|parameter| parameter.name == *base_name)
        else {
            return Ok(false);
        };
        if !matches!(
            base_parameter.parameter_type,
            Type::StructPointer { .. } | Type::Pointer(_)
        ) || !function
            .parameters
            .iter()
            .any(|parameter| parameter.name == *value_name)
        {
            return Ok(false);
        }

        let member_constant = |statement: &Statement| {
            let Statement::Store {
                target:
                    Expression::Member {
                        base,
                        offset,
                        member_type,
                        index_stride: None,
                    },
                value,
            } = statement
            else {
                return None;
            };
            let Expression::Variable(name) = base.as_ref() else {
                return None;
            };
            (name == base_name).then_some((*offset, *member_type, constant_value(value)?))
        };
        let Some((second_offset, second_type, second_value)) = member_constant(second) else {
            return Ok(false);
        };
        let Some((third_offset, third_type, third_value)) = member_constant(third) else {
            return Ok(false);
        };
        if second_value == third_value {
            return Ok(false);
        }
        let Some(first_pointee) = pointee_of_type(*first_type) else {
            return Ok(false);
        };
        let Some(second_pointee) = pointee_of_type(second_type) else {
            return Ok(false);
        };
        let Some(third_pointee) = pointee_of_type(third_type) else {
            return Ok(false);
        };
        if matches!(
            (first_pointee, second_pointee, third_pointee),
            (Pointee::Float | Pointee::Double, _, _)
                | (_, Pointee::Float | Pointee::Double, _)
                | (_, _, Pointee::Float | Pointee::Double)
        ) {
            return Ok(false);
        }
        let (first_offset, second_offset, third_offset) = match (
            i16::try_from(*first_offset),
            i16::try_from(second_offset),
            i16::try_from(third_offset),
        ) {
            (Ok(first), Ok(second), Ok(third)) => (first, second, third),
            _ => return Ok(false),
        };
        let base_register = self.general_register_of_leaf(first_base)?;
        let value = Expression::Variable(value_name.clone());
        let value_register = self.general_register_of_leaf(&value)?;
        if base_register == value_register || value_register == GENERAL_SCRATCH {
            return Ok(false);
        }

        self.output.instructions.push(displacement_store(
            first_pointee,
            value_register,
            base_register,
            first_offset,
        )?);
        self.load_integer_constant(value_register, second_value);
        self.load_integer_constant(GENERAL_SCRATCH, third_value);
        self.output.instructions.push(displacement_store(
            second_pointee,
            value_register,
            base_register,
            second_offset,
        )?);
        self.output.instructions.push(displacement_store(
            third_pointee,
            GENERAL_SCRATCH,
            base_register,
            third_offset,
        )?);
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
