//! Element-wise arithmetic exposed as aggregate member stores.

#[allow(unused_imports)]
use super::*;

fn vec3_scalar_product<'a>(
    target: &'a Expression,
    value: &'a Expression,
) -> Option<(&'a Expression, u32, &'a Expression)> {
    let Expression::Member {
        base,
        offset,
        member_type: Type::Struct { size: 12, .. },
        index_stride: None,
    } = target
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Multiply,
        left,
        right,
    } = value
    else {
        return None;
    };
    if structurally_equal(left, target) {
        Some((base, *offset, right))
    } else if structurally_equal(right, target) {
        Some((base, *offset, left))
    } else {
        None
    }
}

impl Generator {
    /// Lower `vec3_member = vec3_member * scalar` without re-evaluating the
    /// scalar for every lane. Inline expansion retains the aggregate operation
    /// for some vector compound assignments, even though the ABI storage is
    /// three adjacent single-precision values.
    pub(crate) fn try_emit_member_vec3_scalar_product(
        &mut self,
        target: &Expression,
        value: &Expression,
    ) -> Compilation<bool> {
        let Some((base, offset, scalar)) = vec3_scalar_product(target, value) else {
            return Ok(false);
        };

        let scale = self.fresh_virtual_float_preferring(Eabi::float_result().number);
        self.evaluate_float(scalar, scale)?;
        let address = self.member_base_register(base)?;
        let lane = self.fresh_virtual_float_preferring(FLOAT_SCRATCH);
        let offset = i16::try_from(offset)
            .map_err(|_| Diagnostic::error("a Vec3 arithmetic member offset is out of range"))?;

        for lane_offset in [0_i16, 4, 8] {
            let displacement = offset.checked_add(lane_offset).ok_or_else(|| {
                Diagnostic::error("a Vec3 arithmetic member lane is out of range")
            })?;
            self.output.instructions.push(Instruction::LoadFloatSingle {
                d: lane,
                a: address,
                offset: displacement,
            });
            self.output
                .instructions
                .push(Instruction::FloatMultiplySingle {
                    d: lane,
                    a: lane,
                    c: scale,
                });
            self.output
                .instructions
                .push(Instruction::StoreFloatSingle {
                    s: lane,
                    a: address,
                    offset: displacement,
                });
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::vec3_scalar_product;
    use mwcc_syntax_trees::{BinaryOperator, Expression, Type};

    fn member() -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable("object".into())),
            offset: 20,
            member_type: Type::Struct { size: 12, align: 4 },
            index_stride: None,
        }
    }

    #[test]
    fn recognizes_a_vec3_self_product_without_accepting_a_different_source() {
        let target = member();
        let scalar = Expression::Variable("scale".into());
        let product = Expression::Binary {
            operator: BinaryOperator::Multiply,
            left: Box::new(target.clone()),
            right: Box::new(scalar),
        };
        let other_product = Expression::Binary {
            operator: BinaryOperator::Multiply,
            left: Box::new(Expression::Member {
                base: Box::new(Expression::Variable("other".into())),
                offset: 20,
                member_type: Type::Struct { size: 12, align: 4 },
                index_stride: None,
            }),
            right: Box::new(Expression::Variable("scale".into())),
        };

        assert!(vec3_scalar_product(&target, &product).is_some());
        assert!(vec3_scalar_product(&target, &other_product).is_none());
    }
}
