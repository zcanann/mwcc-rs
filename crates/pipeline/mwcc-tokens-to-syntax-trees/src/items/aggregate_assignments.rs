//! Typed scalarization of aggregate value assignments.
//!
//! Aggregate tags and field declarations deliberately do not enter machine IR.
//! This pass expands copies while the parser still knows each field's real load
//! and store class, avoiding size-based guesses in instruction selection.

use crate::parser::Parser;
use mwcc_core::Compilation;
use mwcc_syntax_trees::{Expression, LocalDeclaration, Statement, Type};

struct MaterializedAggregate {
    tag: String,
    value: Expression,
    effects: Vec<Expression>,
}

impl Parser {
    /// Materialize a computed aggregate initializer, then copy the completed
    /// object into its newly declared local. Initialization deliberately does
    /// not call `operator=`: MWCC uses the value-producing call's hidden
    /// temporary followed by an object copy for shapes such as
    /// `Vec difference = left - right`.
    pub(super) fn lower_cxx_aggregate_local_initialization(
        &mut self,
        name: &str,
        value: &Expression,
        declared_tag: Option<&str>,
        value_tag: Option<&str>,
        local_names: &mut std::collections::HashSet<String>,
        block_locals: &mut Vec<LocalDeclaration>,
    ) -> Compilation<Option<Statement>> {
        let (Some(declared_tag), Some(value_tag)) = (declared_tag, value_tag) else {
            return Ok(None);
        };
        if !same_cxx_aggregate_identity(declared_tag, value_tag)
            || is_addressable_aggregate_value(value)
        {
            return Ok(None);
        }
        if let Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } = value
        {
            let saved_expression_tag = self.expression_struct_tag.take();
            let when_true = self.materialize_cxx_aggregate_value(
                when_true,
                Some(value_tag),
                local_names,
                block_locals,
            );
            let when_false = self.materialize_cxx_aggregate_value(
                when_false,
                Some(value_tag),
                local_names,
                block_locals,
            );
            self.expression_struct_tag = saved_expression_tag;
            let (Some(when_true), Some(when_false)) = (when_true?, when_false?) else {
                return Ok(None);
            };
            if !same_cxx_aggregate_identity(declared_tag, &when_true.tag)
                || !same_cxx_aggregate_identity(declared_tag, &when_false.tag)
            {
                return Ok(None);
            }
            let arm = |materialized: MaterializedAggregate| {
                let mut statements = materialized
                    .effects
                    .into_iter()
                    .map(Statement::Expression)
                    .collect::<Vec<_>>();
                statements.push(Statement::Assign {
                    name: name.to_owned(),
                    value: materialized.value,
                });
                statements
            };
            return Ok(Some(Statement::If {
                condition: condition.as_ref().clone(),
                then_body: arm(when_true),
                else_body: arm(when_false),
            }));
        }
        let saved_expression_tag = self.expression_struct_tag.take();
        let materialized = self.materialize_cxx_aggregate_value(
            value,
            Some(value_tag),
            local_names,
            block_locals,
        );
        self.expression_struct_tag = saved_expression_tag;
        let Some(materialized) = materialized? else {
            return Ok(None);
        };
        if !same_cxx_aggregate_identity(declared_tag, &materialized.tag) {
            return Ok(None);
        }
        let mut effects = materialized.effects;
        effects.push(Expression::Assign {
            target: Box::new(Expression::Variable(name.to_owned())),
            value: Box::new(materialized.value),
        });
        Ok(sequence_effects(effects).map(Statement::Expression))
    }

    /// Lower an aggregate assignment through the class's declared `operator=`.
    ///
    /// Addressable values pass directly by reference. Aggregate-valued calls
    /// and overloaded arithmetic receive explicit frame temporaries in source
    /// evaluation order, leaving frame planning and call lowering as the sole
    /// owners of storage and register placement.
    pub(super) fn lower_cxx_overloaded_assignment(
        &mut self,
        target: &Expression,
        value: &Expression,
        target_tag: Option<&str>,
        value_tag: Option<&str>,
        local_names: &mut std::collections::HashSet<String>,
        block_locals: &mut Vec<LocalDeclaration>,
    ) -> Compilation<Option<Statement>> {
        let Some(target_tag) = target_tag else {
            return Ok(None);
        };
        if value_tag.is_some() && is_addressable_aggregate_value(value) {
            return self.lower_direct_cxx_aggregate_assignment(
                target,
                value,
                target_tag,
                value_tag,
            );
        }

        let Some(value_tag) = value_tag else {
            return Ok(None);
        };
        if !same_cxx_aggregate_identity(target_tag, value_tag) {
            return Ok(None);
        }
        let saved_expression_tag = self.expression_struct_tag.take();
        let materialized = self.materialize_cxx_aggregate_value(
            value,
            Some(value_tag),
            local_names,
            block_locals,
        );
        let materialized = match materialized {
            Ok(Some(materialized)) => materialized,
            Ok(None) => {
                self.expression_struct_tag = saved_expression_tag;
                return Ok(None);
            }
            Err(error) => {
                self.expression_struct_tag = saved_expression_tag;
                return Err(error);
            }
        };
        if !same_cxx_aggregate_identity(target_tag, &materialized.tag) {
            self.expression_struct_tag = saved_expression_tag;
            return Ok(None);
        }
        self.expression_struct_tag = Some(materialized.tag);
        let consumer = self.lower_cxx_instance_member_call(
            target_tag,
            "__as",
            target.clone(),
            vec![materialized.value],
        );
        self.expression_struct_tag = saved_expression_tag;
        let consumer = consumer?;
        let mut effects = materialized.effects;
        effects.push(consumer);
        Ok(sequence_effects(effects).map(Statement::Expression))
    }

    fn lower_direct_cxx_aggregate_assignment(
        &mut self,
        target: &Expression,
        value: &Expression,
        target_tag: &str,
        value_tag: Option<&str>,
    ) -> Compilation<Option<Statement>> {
        let saved_expression_tag =
            std::mem::replace(&mut self.expression_struct_tag, value_tag.map(str::to_owned));
        let declared =
            self.resolve_instance_member_call(target_tag, "__as", std::slice::from_ref(value));
        let declared = match declared {
            Ok(declared) => declared,
            Err(error) => {
                self.expression_struct_tag = saved_expression_tag;
                return Err(error);
            }
        };
        if declared.is_none() {
            self.expression_struct_tag = saved_expression_tag;
            return Ok(None);
        }
        let assignment =
            self.lower_cxx_instance_member_call(target_tag, "__as", target.clone(), vec![
                value.clone(),
            ]);
        self.expression_struct_tag = saved_expression_tag;
        assignment.map(|call| Some(Statement::Expression(call)))
    }

    /// Turn one aggregate-valued expression into an addressable value. Nested
    /// overloaded operators recurse through their aggregate operands, so
    /// `vector * scalar + center` becomes two ordered hidden-result calls rather
    /// than a scalar binary tree.
    fn materialize_cxx_aggregate_value(
        &mut self,
        expression: &Expression,
        expected_tag: Option<&str>,
        local_names: &mut std::collections::HashSet<String>,
        block_locals: &mut Vec<LocalDeclaration>,
    ) -> Compilation<Option<MaterializedAggregate>> {
        if is_addressable_aggregate_value(expression) {
            let tag = self
                .cxx_expression_struct_tag(expression)
                .map(str::to_owned)
                .or_else(|| expected_tag.map(str::to_owned));
            return Ok(tag.map(|tag| MaterializedAggregate {
                tag,
                value: expression.clone(),
                effects: Vec::new(),
            }));
        }

        match expression {
            Expression::Call { name, .. } => {
                let tag = self
                    .function_return_structs
                    .get(name)
                    .cloned()
                    .or_else(|| expected_tag.map(str::to_owned));
                let Some(tag) = tag else {
                    return Ok(None);
                };
                self.materialize_cxx_aggregate_producer(
                    expression.clone(),
                    &tag,
                    Vec::new(),
                    local_names,
                    block_locals,
                )
            }
            Expression::VirtualCall {
                return_type: Type::Struct { .. },
                ..
            } => {
                let Some(tag) = expected_tag else {
                    return Ok(None);
                };
                self.materialize_cxx_aggregate_producer(
                    expression.clone(),
                    tag,
                    Vec::new(),
                    local_names,
                    block_locals,
                )
            }
            Expression::Binary {
                operator,
                left,
                right,
            } => {
                let Some(operator_name) = crate::cxx::arithmetic_operator_name(*operator) else {
                    return Ok(None);
                };
                let Some(left) = self.materialize_cxx_aggregate_value(
                    left,
                    None,
                    local_names,
                    block_locals,
                )?
                else {
                    return Ok(None);
                };
                let right_aggregate = self.materialize_cxx_aggregate_value(
                    right,
                    None,
                    local_names,
                    block_locals,
                )?;
                let (right_value, right_tag, right_effects) = match right_aggregate {
                    Some(right) => (right.value, Some(right.tag), right.effects),
                    None => (right.as_ref().clone(), None, Vec::new()),
                };
                self.expression_struct_tag = right_tag;
                let producer = self.lower_cxx_instance_member_call(
                    &left.tag,
                    operator_name,
                    left.value,
                    vec![right_value],
                )?;
                let result_tag = self
                    .expression_struct_tag
                    .take()
                    .or_else(|| expected_tag.map(str::to_owned));
                let Some(result_tag) = result_tag else {
                    return Ok(None);
                };
                let mut effects = left.effects;
                effects.extend(right_effects);
                self.materialize_cxx_aggregate_producer(
                    producer,
                    &result_tag,
                    effects,
                    local_names,
                    block_locals,
                )
            }
            _ => Ok(None),
        }
    }

    fn materialize_cxx_aggregate_producer(
        &mut self,
        producer: Expression,
        tag: &str,
        mut effects: Vec<Expression>,
        local_names: &mut std::collections::HashSet<String>,
        block_locals: &mut Vec<LocalDeclaration>,
    ) -> Compilation<Option<MaterializedAggregate>> {
        let resolved = self
            .resolve_scoped_cxx_class_name(tag)
            .unwrap_or_else(|| tag.to_owned());
        let Some(layout) = self
            .structs
            .get(&resolved)
            .or_else(|| self.structs.get(tag))
        else {
            return Ok(None);
        };
        let temporary_type = Type::Struct {
            size: layout.size,
            align: layout.align,
        };
        let mut temporary_index = block_locals.len();
        let temporary = loop {
            let candidate = format!("__mwcc_aggregate_result_{temporary_index}");
            if !local_names.contains(&candidate) {
                break candidate;
            }
            temporary_index += 1;
        };
        local_names.insert(temporary.clone());
        self.variable_types
            .insert(temporary.clone(), temporary_type);
        self.variable_structs
            .insert(temporary.clone(), resolved.clone());
        block_locals.push(LocalDeclaration {
            declared_type: temporary_type,
            name: temporary.clone(),
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            row_bytes: None,
        });
        effects.push(Expression::Assign {
            target: Box::new(Expression::Variable(temporary.clone())),
            value: Box::new(producer),
        });
        Ok(Some(MaterializedAggregate {
            tag: resolved,
            value: Expression::Variable(temporary),
            effects,
        }))
    }

    /// Copy declared scalar fields in source order and recurse through embedded
    /// aggregates. Padding is not an object value and is therefore untouched.
    pub(super) fn lower_typed_aggregate_assignment(
        &self,
        target: &Expression,
        value: &Expression,
        target_tag: Option<&str>,
        value_tag: Option<&str>,
    ) -> Option<Expression> {
        // Ordinary C++ assignment may dispatch a user-defined operator=. This
        // scalarizer serves recovered skipped-inline bodies, whose raw field
        // assignment must be composed at their call sites; leave ordinary
        // function bodies unchanged until overload resolution owns that choice.
        if !self.recover_skipped_inline_definition {
            return None;
        }
        // A pointer/reference expression carries its pointee's struct tag for
        // overload resolution, but that does not make the pointer itself an
        // aggregate value. Require aggregate storage on both sides before
        // expanding fields; otherwise `member_ptr = parameter_ptr` would be
        // miscompiled as `*member_ptr = *parameter_ptr`.
        let is_aggregate_value = |expression: &Expression| {
            matches!(
                self.cxx_expression_type(expression),
                Some(Type::Struct { .. })
            ) || matches!(expression, Expression::Variable(name)
                    if self.cxx_reference_variables.contains(name))
        };
        if !is_aggregate_value(target) || !is_aggregate_value(value) {
            return None;
        }
        let target_tag = target_tag?;
        let value_tag = value_tag?;
        if target_tag != value_tag {
            return None;
        }
        let mut active = std::collections::HashSet::new();
        let assignments =
            self.scalar_aggregate_copy_fields(target_tag, target, value, &mut active)?;
        let mut assignments = assignments.into_iter();
        let first = assignments.next()?;
        Some(assignments.fold(first, |left, right| Expression::Comma {
            left: Box::new(left),
            right: Box::new(right),
        }))
    }

    fn scalar_aggregate_copy_fields(
        &self,
        tag: &str,
        target: &Expression,
        value: &Expression,
        active: &mut std::collections::HashSet<String>,
    ) -> Option<Vec<Expression>> {
        if !active.insert(tag.to_owned()) {
            return None;
        }
        let layout = self.structs.get(tag)?;
        if layout.is_union {
            active.remove(tag);
            return None;
        }
        let fields = layout
            .fields_in_declaration_order()
            .into_iter()
            .map(|(_, field)| field.clone())
            .collect::<Vec<_>>();
        let mut assignments = Vec::new();
        for field in fields {
            if field.array_element.is_some()
                || field.array_bytes.is_some()
                || field.bit_field.is_some()
            {
                active.remove(tag);
                return None;
            }
            let target_field = Expression::Member {
                base: Box::new(target.clone()),
                offset: field.offset,
                member_type: field.member_type,
                index_stride: None,
            };
            let value_field = Expression::Member {
                base: Box::new(value.clone()),
                offset: field.offset,
                member_type: field.member_type,
                index_stride: None,
            };
            if matches!(field.member_type, Type::Struct { .. }) {
                let nested = field.struct_tag.as_deref()?;
                assignments.extend(self.scalar_aggregate_copy_fields(
                    nested,
                    &target_field,
                    &value_field,
                    active,
                )?);
            } else if matches!(
                field.member_type,
                Type::Int
                    | Type::UnsignedInt
                    | Type::Char
                    | Type::UnsignedChar
                    | Type::Short
                    | Type::UnsignedShort
                    | Type::Float
                    | Type::Double
                    | Type::Pointer(_)
                    | Type::StructPointer { .. }
            ) {
                assignments.push(Expression::Assign {
                    target: Box::new(target_field),
                    value: Box::new(value_field),
                });
            } else {
                active.remove(tag);
                return None;
            }
        }
        active.remove(tag);
        Some(assignments)
    }
}

fn is_addressable_aggregate_value(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Variable(_)
            | Expression::Member {
                member_type: Type::Struct { .. },
                ..
            }
            | Expression::Index { .. }
            | Expression::Dereference { .. }
    )
}

fn same_cxx_aggregate_identity(left: &str, right: &str) -> bool {
    left == right || left.rsplit("::").next() == right.rsplit("::").next()
}

fn sequence_effects(mut effects: Vec<Expression>) -> Option<Expression> {
    let last = effects.pop()?;
    Some(effects.into_iter().rev().fold(last, |right, left| {
        Expression::Comma {
            left: Box::new(left),
            right: Box::new(right),
        }
    }))
}
