//! Linkage-first stack layout for addressable variadic output values.
//!
//! When a variadic call receives pointers to scalar locals, MWCC keeps those
//! output slots above its incoming argument-home table. A later automatic
//! array may reuse the table's final word, while numeric-conversion scratch
//! follows the output slots. This plan owns those related offsets without
//! leaking a semantic call shape into generic frame arithmetic.

use super::structured_expression_visit::visit_statement;
use mwcc_syntax_trees::{Expression, Function, LocalDeclaration};
use std::collections::HashSet;

const ARGUMENT_HOME_END: i16 = 40;

pub(super) struct StructuredVariadicOutputFrame {
    pub(super) array_offset: i16,
    pub(super) scalar_offset: i16,
    pub(super) local_end: i16,
    pub(super) conversion_base: i16,
}

impl StructuredVariadicOutputFrame {
    pub(super) fn plan(
        function: &Function,
        frame_arrays: &[&LocalDeclaration],
        frame_array_bytes: i16,
        frame_scalar_locals: &[&LocalDeclaration],
        frame_scalar_parameters: usize,
        aggregate_count: usize,
        int_to_float_conversion_count: usize,
        variadic_callees: &HashSet<String>,
    ) -> Option<Self> {
        if frame_arrays.len() != 1
            || !(1..=4).contains(&frame_array_bytes)
            || frame_scalar_locals.is_empty()
            || frame_scalar_parameters != 0
            || aggregate_count != 0
            || int_to_float_conversion_count == 0
        {
            return None;
        }

        let scalar_names: HashSet<String> = frame_scalar_locals
            .iter()
            .map(|local| local.name.clone())
            .collect();
        let mut addressed_by_variadic = HashSet::new();
        for statement in &function.statements {
            visit_statement(statement, &mut |expression| {
                let Expression::Call { name, arguments } = expression else {
                    return;
                };
                if !variadic_callees.contains(name) {
                    return;
                }
                for argument in arguments {
                    if let Expression::AddressOf { operand } = argument {
                        if let Expression::Variable(name) = operand.as_ref() {
                            if scalar_names.contains(name) {
                                addressed_by_variadic.insert(name.clone());
                            }
                        }
                    }
                }
            });
        }
        if addressed_by_variadic != scalar_names {
            return None;
        }

        let array_offset = ARGUMENT_HOME_END.checked_sub(4)?;
        let scalar_offset = ARGUMENT_HOME_END;
        let scalar_bytes = i16::try_from(frame_scalar_locals.len().checked_mul(4)?).ok()?;
        let local_end = scalar_offset.checked_add(scalar_bytes)?;
        let conversion_base = local_end.checked_add(7)? & !7;
        Some(Self {
            array_offset,
            scalar_offset,
            local_end,
            conversion_base,
        })
    }

    pub(super) fn saved_home_preference(
        &self,
        eager_count: usize,
        parameter_count: usize,
        total_count: usize,
        home_index: usize,
    ) -> Option<u8> {
        (eager_count == 2 && parameter_count == 2 && total_count == 4)
            .then(|| [30, 31, 29, 28].get(home_index).copied())
            .flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{LocalDataRelocation, Statement, Type};

    fn local(name: &str, array_length: Option<u16>) -> LocalDeclaration {
        LocalDeclaration {
            declared_type: if array_length.is_some() {
                Type::Char
            } else {
                Type::Int
            },
            name: name.into(),
            initializer: None,
            is_volatile: false,
            array_length,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::<LocalDataRelocation>::new(),
            is_const: false,
            attribute_alignment: None,
            row_bytes: None,
        }
    }

    fn function(callee: &str) -> Function {
        Function {
            return_type: Type::Void,
            name: "compiled".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: vec![local("output", None), local("text", Some(4))],
            statements: vec![Statement::Expression(Expression::Call {
                name: callee.into(),
                arguments: vec![Expression::AddressOf {
                    operand: Box::new(Expression::Variable("output".into())),
                }],
            })],
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        }
    }

    #[test]
    fn places_variadic_outputs_above_the_argument_home_table() {
        let function = function("report");
        let plan = StructuredVariadicOutputFrame::plan(
            &function,
            &[&function.locals[1]],
            4,
            &[&function.locals[0]],
            0,
            0,
            1,
            &HashSet::from(["report".into()]),
        )
        .unwrap();

        assert_eq!(plan.array_offset, 36);
        assert_eq!(plan.scalar_offset, 40);
        assert_eq!(plan.local_end, 44);
        assert_eq!(plan.conversion_base, 48);
        assert_eq!(plan.saved_home_preference(2, 2, 4, 0), Some(30));
        assert_eq!(plan.saved_home_preference(2, 2, 4, 1), Some(31));
    }

    #[test]
    fn rejects_nonvariadic_output_calls() {
        let function = function("ordinary");
        assert!(StructuredVariadicOutputFrame::plan(
            &function,
            &[&function.locals[1]],
            4,
            &[&function.locals[0]],
            0,
            0,
            1,
            &HashSet::from(["report".into()]),
        )
        .is_none());
    }
}
