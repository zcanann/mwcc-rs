//! Struct-member copy initialization followed by one direct call.
//!
//! The source and destination pointers stay live across the copy run while a
//! constant sibling value occupies r0. Each copied member receives a distinct
//! short-lived virtual so allocation can reuse the source pointer's register
//! after its final load, reproducing MWCC without naming physical temporaries.

#[allow(unused_imports)]
use super::*;

struct MemberCopy<'a> {
    source_base: &'a Expression,
    source_offset: u32,
    target_offset: i16,
    member_type: Type,
    pointee: Pointee,
}

impl Generator {
    pub(crate) fn try_member_copy_then_call(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if function.return_type != Type::Void
            || function.return_expression.is_some()
            || !function.guards.is_empty()
            || !function.locals.is_empty()
        {
            return Ok(false);
        }
        let Some((Statement::Expression(trailing_call @ Expression::Call { .. }), leading)) =
            function.statements.split_last()
        else {
            return Ok(false);
        };
        let Some((constant_statement, copy_statements)) = leading.split_last() else {
            return Ok(false);
        };
        if copy_statements.is_empty() {
            return Ok(false);
        }
        let Statement::Store {
            target:
                Expression::Member {
                    base: constant_base,
                    offset: constant_offset,
                    member_type: constant_type,
                    index_stride: None,
                },
            value: constant_value_expression,
        } = constant_statement
        else {
            return Ok(false);
        };
        let Some(constant) = constant_value(constant_value_expression) else {
            return Ok(false);
        };
        let Some(constant_pointee) = pointee_of_type(*constant_type) else {
            return Ok(false);
        };
        if matches!(constant_pointee, Pointee::Float | Pointee::Double)
            || i16::try_from(*constant_offset).is_err()
        {
            return Ok(false);
        }
        let Expression::Variable(target_name) = constant_base.as_ref() else {
            return Ok(false);
        };
        if !expression_reads_name(trailing_call, target_name) {
            return Ok(false);
        }

        let mut source_name: Option<&str> = None;
        let mut copies = Vec::with_capacity(copy_statements.len());
        for statement in copy_statements {
            let Statement::Store {
                target:
                    Expression::Member {
                        base: target_base,
                        offset: target_offset,
                        member_type: target_type,
                        index_stride: None,
                    },
                value:
                    Expression::Member {
                        base: source_base,
                        offset: source_offset,
                        member_type: source_type,
                        index_stride: None,
                    },
            } = statement
            else {
                return Ok(false);
            };
            if target_type != source_type
                || !matches!(target_base.as_ref(), Expression::Variable(name) if name == target_name)
            {
                return Ok(false);
            }
            let Expression::Variable(copy_source_name) = source_base.as_ref() else {
                return Ok(false);
            };
            if source_name
                .replace(copy_source_name)
                .is_some_and(|name| name != copy_source_name)
            {
                return Ok(false);
            }
            let Some(pointee) = pointee_of_type(*target_type) else {
                return Ok(false);
            };
            if matches!(pointee, Pointee::Float | Pointee::Double)
                || i16::try_from(*target_offset).is_err()
            {
                return Ok(false);
            }
            copies.push(MemberCopy {
                source_base,
                source_offset: *source_offset,
                target_offset: *target_offset as i16,
                member_type: *target_type,
                pointee,
            });
        }
        let Some(source_name) = source_name else {
            return Ok(false);
        };
        let (Some(target), Some(source)) = (
            self.locations.get(target_name),
            self.locations.get(source_name),
        ) else {
            return Ok(false);
        };
        if target.class != ValueClass::General || source.class != ValueClass::General {
            return Ok(false);
        }
        let target_register = target.register;

        self.output.pre_scheduled = true;
        self.emit_plain_nonleaf_prologue();
        self.load_integer_constant(0, constant);
        for copy in copies {
            let temporary = self.fresh_virtual_general();
            self.emit_member_load(
                copy.source_base,
                copy.source_offset,
                copy.member_type,
                None,
                temporary,
            )?;
            self.output.instructions.push(displacement_store(
                copy.pointee,
                temporary,
                target_register,
                copy.target_offset,
            )?);
        }
        self.output.instructions.push(displacement_store(
            constant_pointee,
            0,
            target_register,
            *constant_offset as i16,
        )?);
        self.emit_statement(function.statements.last().unwrap())?;
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
