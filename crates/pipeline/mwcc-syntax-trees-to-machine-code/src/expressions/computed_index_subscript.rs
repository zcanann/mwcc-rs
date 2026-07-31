//! Member-pointer subscripts whose index is another loaded member.
//!
//! MWCC loads the pointer into the result register, loads and scales the index
//! through r0, then performs the indexed load. Both aggregate base registers
//! remain intact for later use.

#[allow(unused_imports)]
use super::*;

fn member_pointer_and_index<'a>(
    base: &'a Expression,
    index: &'a Expression,
) -> Option<(Pointee, &'a Expression, &'a Expression)> {
    let Expression::Member {
        base: pointer_owner,
        member_type: Type::Pointer(pointee),
        index_stride: None,
        ..
    } = base
    else {
        return None;
    };
    let Expression::Member {
        base: index_owner,
        member_type:
            Type::Char
            | Type::UnsignedChar
            | Type::Short
            | Type::UnsignedShort
            | Type::Int
            | Type::UnsignedInt,
        index_stride: None,
        ..
    } = index
    else {
        return None;
    };
    Some((*pointee, pointer_owner, index_owner))
}

fn member_pointer_and_affine_global_index<'a>(
    base: &'a Expression,
    index: &'a Expression,
) -> Option<(Pointee, &'a str)> {
    let Expression::Member {
        member_type: Type::Pointer(pointee),
        index_stride: None,
        ..
    } = base
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left,
        right,
    } = index
    else {
        return None;
    };
    match (left.as_ref(), right.as_ref()) {
        (Expression::Variable(global), constant)
            if constant_value(constant).is_some() => Some((*pointee, global)),
        (constant, Expression::Variable(global))
            if constant_value(constant).is_some() => Some((*pointee, global)),
        _ => None,
    }
}

impl Generator {
    pub(crate) fn try_emit_computed_index_member_pointer_subscript(
        &mut self,
        base: &Expression,
        index: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        if let Some((pointee, global)) =
            member_pointer_and_affine_global_index(base, index)
        {
            if destination == GENERAL_SCRATCH
                || matches!(
                    pointee,
                    Pointee::Float
                        | Pointee::Double
                        | Pointee::LongLong
                        | Pointee::UnsignedLongLong
                )
                || !matches!(
                    self.globals.get(global),
                    Some(
                        Type::Char
                            | Type::UnsignedChar
                            | Type::Short
                            | Type::UnsignedShort
                            | Type::Int
                            | Type::UnsignedInt
                    )
                )
            {
                return Ok(false);
            }
            // MWCC retains the loaded member pointer independently of the
            // saved result, computes `global + constant` in the next volatile
            // lane, and scales through r0 before the indexed load.
            let pointer = self.fresh_virtual_general_preferring(5);
            self.evaluate_general(base, pointer)?;
            let restore = self.reserved.insert(pointer);
            let affine = self.fresh_virtual_general_preferring(4);
            self.evaluate_general(index, affine)?;
            let scaled = if pointee.size() == 1 {
                affine
            } else {
                self.output.instructions.push(Instruction::ShiftLeftImmediate {
                    a: GENERAL_SCRATCH,
                    s: affine,
                    shift: pointee.size().trailing_zeros() as u8,
                });
                GENERAL_SCRATCH
            };
            if restore {
                self.reserved.remove(&pointer);
            }
            self.output.instructions.push(indexed_load(
                pointee,
                destination,
                pointer,
                scaled,
            )?);
            return Ok(true);
        }
        let Some((pointee, pointer_owner, index_owner)) = member_pointer_and_index(base, index)
        else {
            return Ok(false);
        };
        if destination == GENERAL_SCRATCH
            || matches!(
                pointee,
                Pointee::Float | Pointee::Double | Pointee::LongLong | Pointee::UnsignedLongLong
            )
        {
            return Ok(false);
        }
        let pointer_owner_register = self.general_register_of_leaf(pointer_owner)?;
        let index_owner_register = self.general_register_of_leaf(index_owner)?;
        if destination == pointer_owner_register || destination == index_owner_register {
            return Ok(false);
        }

        self.evaluate_general(base, destination)?;
        let restore = self.reserved.insert(destination);
        self.evaluate_general(index, GENERAL_SCRATCH)?;
        let size = pointee.size();
        if size > 1 {
            self.output
                .instructions
                .push(Instruction::ShiftLeftImmediate {
                    a: GENERAL_SCRATCH,
                    s: GENERAL_SCRATCH,
                    shift: size.trailing_zeros() as u8,
                });
        }
        if restore {
            self.reserved.remove(&destination);
        }
        self.output.instructions.push(indexed_load(
            pointee,
            destination,
            destination,
            GENERAL_SCRATCH,
        )?);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_member_index_over_a_member_pointer() {
        let pointer = Expression::Member {
            base: Box::new(Expression::Variable("object".into())),
            offset: 72,
            member_type: Type::Pointer(Pointee::Short),
            index_stride: None,
        };
        let index = Expression::Member {
            base: Box::new(Expression::Variable("data".into())),
            offset: 28,
            member_type: Type::Short,
            index_stride: None,
        };

        assert!(member_pointer_and_index(&pointer, &index).is_some());
        assert!(member_pointer_and_index(&pointer, &Expression::IntegerLiteral(0)).is_none());
    }

    #[test]
    fn recognizes_an_affine_global_index_over_a_member_pointer() {
        let pointer = Expression::Member {
            base: Box::new(Expression::Variable("object".into())),
            offset: 64,
            member_type: Type::Pointer(Pointee::Short),
            index_stride: None,
        };
        let index = Expression::Binary {
            operator: BinaryOperator::Add,
            left: Box::new(Expression::Variable("selected".into())),
            right: Box::new(Expression::IntegerLiteral(1)),
        };

        assert_eq!(
            member_pointer_and_affine_global_index(&pointer, &index),
            Some((Pointee::Short, "selected"))
        );
        assert!(member_pointer_and_affine_global_index(
            &pointer,
            &Expression::Variable("selected".into()),
        )
        .is_none());
    }
}
