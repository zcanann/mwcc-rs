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
            if matches!(allocated_type, Type::Struct { .. }) {
                return Err(Diagnostic::error(
                    "C++ class array new needs element construction and an array cookie (roadmap)",
                ));
            }
            let element_bytes = allocation_bytes(allocated_type)?;
            let bytes = scale_allocation_count(count, element_bytes);
            return Ok(Expression::Call {
                name: "__nwa__FUl".to_owned(),
                arguments: vec![bytes],
            });
        }

        if let Some(class_name) = aggregate_tag {
            return Err(Diagnostic::error(format!(
                "constructed scalar C++ new for class '{class_name}' needs the allocation null guard (roadmap)"
            )));
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
        let class_name = self.parse_identifier()?;
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
        let constructor = self.resolve_placement_constructor(&class_name, arguments.len())?;
        let mut call_arguments = placement_arguments;
        call_arguments.extend(arguments);
        self.expression_struct_tag = Some(class_name);
        Ok(Expression::Call {
            name: constructor,
            arguments: call_arguments,
        })
    }
}

fn allocation_bytes(allocated_type: Type) -> Compilation<u8> {
    match allocated_type {
        Type::Void => Err(Diagnostic::error("cannot allocate an object of type void")),
        Type::Struct { .. } => Err(Diagnostic::error(
            "C++ class allocation needs constructor-aware lowering (roadmap)",
        )),
        other => Ok(other.width() / 8),
    }
}

fn scale_allocation_count(count: Expression, element_bytes: u8) -> Expression {
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
