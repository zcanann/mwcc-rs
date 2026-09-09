//! Address calculations keep element stride distinct from scalar load width.
use super::*;

fn stride(ty: Type) -> Option<u32> {
    match ty {
        Type::Pointer(element) => Some(u32::from(element.size())),
        Type::StructPointer { element_size } if element_size != 0 => Some(element_size),
        _ => None,
    }
}

fn pointer_type(ty: Type) -> Option<Type> {
    Some(Type::Pointer(match ty {
        Type::Char => Pointee::Char,
        Type::UnsignedChar => Pointee::UnsignedChar,
        Type::Short => Pointee::Short,
        Type::UnsignedShort => Pointee::UnsignedShort,
        Type::Int => Pointee::Int,
        Type::UnsignedInt => Pointee::UnsignedInt,
        Type::Pointer(_) | Type::StructPointer { .. } => Pointee::Pointer,
        Type::LongLong => Pointee::LongLong,
        Type::UnsignedLongLong => Pointee::UnsignedLongLong,
        Type::Struct { size, .. } => return Some(Type::StructPointer { element_size: size }),
        _ => return None,
    }))
}

impl Graph<'_> {
    fn offset_index(
        &mut self,
        pointer: Value,
        index: Value,
        bytes: u32,
        subtract: bool,
    ) -> Option<Value> {
        if !supported(index.ty) || matches!(index.ty, Type::Pointer(_) | Type::StructPointer { .. })
        {
            return None;
        }
        let mut offset = self.convert(index, Type::Int)?;
        if bytes != 1 {
            if let Source::Constant(value) = offset.source {
                offset.source = Source::Constant(u64::from((value as u32).wrapping_mul(bytes)));
            } else {
                let result = self.fresh(Type::Int);
                self.operations.push(Operation::Binary {
                    retain_pair_carry: false,
                    result,
                    operator: BinaryOperator::Multiply,
                    left: offset,
                    right: Value {
                        ty: Type::Int,
                        source: Source::Constant(u64::from(bytes)),
                    },
                });
                offset = result;
            }
        }
        let result = self.fresh(pointer.ty);
        self.operations.push(Operation::Binary {
            retain_pair_carry: false,
            result,
            operator: if subtract {
                BinaryOperator::Subtract
            } else {
                BinaryOperator::Add
            },
            left: pointer,
            right: offset,
        });
        Some(result)
    }

    pub(super) fn pointer_arithmetic(
        &mut self,
        operator: BinaryOperator,
        left: Value,
        right: Value,
    ) -> Option<Value> {
        match (stride(left.ty), stride(right.ty), operator) {
            (Some(bytes), None, BinaryOperator::Add | BinaryOperator::Subtract) => {
                self.offset_index(left, right, bytes, operator == BinaryOperator::Subtract)
            }
            (None, Some(bytes), BinaryOperator::Add) => {
                self.offset_index(right, left, bytes, false)
            }
            _ => None,
        }
    }

    /// An array row or aggregate index decays to an address; scalar indices load.
    /// Retain the frontend's multidimensional member stride for the first index.
    pub(super) fn index_address(
        &mut self,
        base: &Expression,
        index: &Expression,
    ) -> Option<(Value, Option<Type>)> {
        let (pointer, bytes, element) = if let Expression::MemberAddress {
            base,
            offset,
            element,
            index_stride: Some(bytes),
        } = base
        {
            let pointer = self.member_address(base, *offset)?;
            (
                Value {
                    ty: Type::Pointer(*element),
                    ..pointer
                },
                *bytes,
                None,
            )
        } else {
            let pointer = self.expression(base)?;
            let bytes = stride(pointer.ty)?;
            let element = match pointer.ty {
                Type::Pointer(element) => Some(element.element()),
                Type::StructPointer { .. } => None,
                _ => return None,
            };
            (pointer, bytes, element)
        };
        let index = self.expression(index)?;
        Some((self.offset_index(pointer, index, bytes, false)?, element))
    }

    pub(super) fn lvalue(&mut self, target: &Expression) -> Option<(Value, Type)> {
        match target {
            Expression::Variable(name) => {
                let ty = *self.types.get(name).or_else(|| self.globals.get(name))?;
                let pointer = self.address(name)?;
                Some((
                    Value {
                        ty: pointer_type(ty)?,
                        ..pointer
                    },
                    ty,
                ))
            }
            Expression::Dereference { pointer } => {
                let pointer = self.expression(pointer)?;
                let Type::Pointer(element) = pointer.ty else {
                    return None;
                };
                Some((pointer, element.element()))
            }
            Expression::Member {
                base,
                offset,
                member_type,
                ..
            } => {
                let pointer = self.member_address(base, *offset)?;
                Some((
                    Value {
                        ty: pointer_type(*member_type)?,
                        ..pointer
                    },
                    *member_type,
                ))
            }
            Expression::Index { base, index } => {
                let (pointer, element) = self.index_address(base, index)?;
                Some((pointer, element?))
            }
            _ => None,
        }
    }
}
