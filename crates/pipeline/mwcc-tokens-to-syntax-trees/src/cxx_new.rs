//! C++ allocation-expression parsing and ABI normalization.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{BinaryOperator, Expression, Type};
use mwcc_tokens::Token;

use crate::parser::Parser;

impl Parser {
    /// Parse the operand following the already-consumed `new` keyword.
    ///
    /// Trivial scalar and array allocations are ordinary EABI calls. Class
    /// construction remains distinct because it also needs a null guard,
    /// single-evaluation storage, and a constructor call in the backend.
    pub(crate) fn parse_cxx_new_expression(&mut self) -> Compilation<Expression> {
        if self.eat_keyword(Token::ParenOpen) {
            return self.parse_placement_new();
        }

        let allocated_type = self.parse_type()?;
        let aggregate_tag = self.last_struct_tag.clone();
        if self.eat_keyword(Token::BracketOpen) {
            let count = self.expression()?;
            self.expect(Token::BracketClose)?;
            let element_bytes =
                self.array_element_bytes(allocated_type, aggregate_tag.as_deref())?;
            let bytes = scale_allocation_count(count, element_bytes);
            return Ok(Expression::Call {
                name: "__nwa__FUl".to_owned(),
                arguments: vec![bytes],
            });
        }

        if let Some(class_name) = aggregate_tag {
            let Type::Struct {
                size: allocation_size,
                ..
            } = allocated_type
            else {
                return Err(Diagnostic::error(
                    "internal: a constructed C++ allocation has no class layout",
                ));
            };
            let mut arguments = Vec::new();
            if self.eat_keyword(Token::ParenOpen) {
                if *self.peek() != Token::ParenClose {
                    loop {
                        arguments.push(self.expression()?);
                        if !self.eat_keyword(Token::Comma) {
                            break;
                        }
                    }
                }
                self.expect(Token::ParenClose)?;
            }
            let constructor = self.resolve_placement_constructor(&class_name, &arguments)?;
            self.expression_struct_tag = Some(class_name);
            return Ok(Expression::ConstructedNew {
                allocation: Box::new(Expression::Call {
                    name: "__nw__FUl".to_owned(),
                    arguments: vec![Expression::IntegerLiteral(i64::from(allocation_size))],
                }),
                allocation_size,
                constructor,
                arguments,
            });
        }
        if *self.peek() == Token::ParenOpen {
            return Err(Diagnostic::error(
                "initialized scalar C++ new needs a post-allocation store (roadmap)",
            ));
        }
        Ok(Expression::Call {
            name: "__nw__FUl".to_owned(),
            arguments: vec![Expression::IntegerLiteral(i64::from(allocation_bytes(
                allocated_type,
            )?))],
        })
    }

    fn parse_placement_new(&mut self) -> Compilation<Expression> {
        let mut placement_arguments = vec![self.expression()?];
        while self.eat_keyword(Token::Comma) {
            placement_arguments.push(self.expression()?);
        }
        self.expect(Token::ParenClose)?;

        let allocated_type = self.parse_type()?;
        let aggregate_tag = self.last_struct_tag.clone();
        if self.eat_keyword(Token::BracketOpen) {
            let count = self.expression()?;
            self.expect(Token::BracketClose)?;
            let bytes = scale_allocation_count(
                count,
                self.array_element_bytes(allocated_type, aggregate_tag.as_deref())?,
            );
            let allocator = self.resolve_placement_array_allocator(&placement_arguments)?;
            let mut arguments = vec![bytes];
            arguments.append(&mut placement_arguments);
            return Ok(Expression::Call {
                name: allocator.to_owned(),
                arguments,
            });
        }

        let class_name = aggregate_tag.ok_or_else(|| {
            Diagnostic::error(
                "initialized scalar C++ placement new needs a class constructor (roadmap)",
            )
        })?;
        self.expect(Token::ParenOpen)?;
        let mut arguments = Vec::new();
        if *self.peek() != Token::ParenClose {
            loop {
                arguments.push(self.expression()?);
                if !self.eat_keyword(Token::Comma) {
                    break;
                }
            }
        }
        self.expect(Token::ParenClose)?;
        if placement_arguments.len() != 1 {
            return Err(Diagnostic::error(format!(
                "constructed scalar C++ placement new for class '{class_name}' with {} placement arguments needs allocator and null-guard lowering (roadmap)",
                placement_arguments.len()
            )));
        }
        let constructor = if arguments.is_empty() {
            self.ensure_implicit_default_constructor(&class_name)?
                .map(Ok)
                .unwrap_or_else(|| self.resolve_placement_constructor(&class_name, &arguments))?
        } else {
            self.resolve_placement_constructor(&class_name, &arguments)?
        };
        let Type::Struct {
            size: allocation_size,
            ..
        } = allocated_type
        else {
            return Err(Diagnostic::error(
                "internal: a placement-constructed C++ class has no layout",
            ));
        };
        let allocation = placement_arguments
            .pop()
            .expect("single placement argument was checked");
        self.expression_struct_tag = Some(class_name);
        Ok(Expression::ConstructedNew {
            allocation: Box::new(allocation),
            allocation_size,
            constructor,
            arguments,
        })
    }

    /// Select the measured CodeWarrior allocation overload for a placement
    /// array. Keep overload policy at the C++ allocation boundary: the general
    /// call parser should not need to know that the allocation size is an
    /// implicit first argument.
    fn resolve_placement_array_allocator(
        &self,
        placement_arguments: &[Expression],
    ) -> Compilation<&'static str> {
        match placement_arguments {
            [alignment]
                if matches!(
                    self.cxx_expression_type(alignment),
                    Some(Type::Int | Type::UnsignedInt)
                ) =>
            {
                Ok("__nwa__FUli")
            }
            [heap, alignment]
                if matches!(
                    self.cxx_expression_type(alignment),
                    Some(Type::Int | Type::UnsignedInt)
                ) && self
                    .cxx_expression_struct_tag(heap)
                    .is_some_and(|class| self.cxx_class_is_or_derives_from(class, "JKRHeap")) =>
            {
                Ok("__nwa__FUlP7JKRHeapi")
            }
            _ => Err(Diagnostic::error(format!(
                "C++ placement array new with {} placement arguments has no recovered allocator overload (roadmap; types {:?}, aggregate {:?})",
                placement_arguments.len(),
                placement_arguments
                    .iter()
                    .map(|argument| self.cxx_expression_type(argument))
                    .collect::<Vec<_>>(),
                placement_arguments
                    .first()
                    .and_then(|argument| self.cxx_expression_struct_tag(argument)),
            ))),
        }
    }

    fn cxx_class_is_or_derives_from(&self, class: &str, target: &str) -> bool {
        let local = class.rsplit("::").next().unwrap_or(class);
        if class == target
            || local == target
            // JKRHeap.h's full JKRSolidHeap body is commonly unavailable after
            // PCH recovery, but the SDK allocation overload is declared against
            // its public JKRHeap base. Retain that stable SDK relationship even
            // when only the forward-declared derived pointer survives.
            || (target == "JKRHeap" && local == "JKRSolidHeap")
        {
            return true;
        }
        let Some(layout) = self.cxx_classes.get(class).or_else(|| {
            self.cxx_classes.get(local)
        }) else {
            return false;
        };
        layout
            .bases
            .iter()
            .any(|base| self.cxx_class_is_or_derives_from(&base.name, target))
    }

    /// Return the allocation stride when an array element needs no constructor,
    /// destructor, vptr, base, or class-valued member lifetime work.
    fn array_element_bytes(
        &self,
        allocated_type: Type,
        aggregate_tag: Option<&str>,
    ) -> Compilation<u32> {
        let Type::Struct { size, .. } = allocated_type else {
            return allocation_bytes(allocated_type);
        };
        let tag = aggregate_tag.ok_or_else(|| {
            Diagnostic::error("C++ class array allocation has no aggregate identity")
        })?;
        let class = self.cxx_classes.get(tag).ok_or_else(|| {
            Diagnostic::error(format!(
                "C++ class array new for '{tag}' needs recovered lifetime information (roadmap)"
            ))
        })?;
        let layout = self.structs.get(tag).ok_or_else(|| {
            Diagnostic::error(format!(
                "C++ class array new for '{tag}' needs a recovered layout (roadmap)"
            ))
        })?;
        let has_source_constructor = self
            .cxx_constructors
            .get(tag)
            .is_some_and(|constructors| !constructors.is_empty());
        let has_class_value_member = layout
            .fields
            .values()
            .any(|field| matches!(field.member_type, Type::Struct { .. }));
        if !class.bases.is_empty()
            || !class.constructors.is_empty()
            || has_source_constructor
            || class.declares_destructor
            || class.is_polymorphic
            || has_class_value_member
        {
            return Err(Diagnostic::error(format!(
                "C++ class array new for '{tag}' needs element construction and an array cookie (roadmap)"
            )));
        }
        Ok(size)
    }
}

fn allocation_bytes(allocated_type: Type) -> Compilation<u32> {
    match allocated_type {
        Type::Void => Err(Diagnostic::error("cannot allocate an object of type void")),
        Type::Struct { .. } => Err(Diagnostic::error(
            "C++ class allocation needs constructor-aware lowering (roadmap)",
        )),
        other => Ok(u32::from(other.width() / 8)),
    }
}

fn scale_allocation_count(count: Expression, element_bytes: u32) -> Expression {
    if element_bytes == 1 {
        count
    } else if let Expression::IntegerLiteral(count) = count {
        Expression::IntegerLiteral(count.wrapping_mul(i64::from(element_bytes)))
    } else {
        Expression::Binary {
            operator: BinaryOperator::Multiply,
            left: Box::new(count),
            right: Box::new(Expression::IntegerLiteral(i64::from(element_bytes))),
        }
    }
}
