//! Parameter-declarator normalization.
//!
//! C adjusts an array parameter to a pointer, while retaining every trailing
//! array extent as part of the pointee type. The executable type model has no
//! array node, so record a two-dimensional parameter's row stride separately
//! for subscript desugaring.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{Expression, Type};
use mwcc_tokens::Token;

use crate::parser::Parser;

use super::pointee_of;

impl Parser {
    pub(super) fn parse_array_parameter_suffix(
        &mut self,
        name: &str,
        element_type: Type,
        array_typedef: Option<(Type, u16, u16)>,
    ) -> Compilation<Type> {
        if *self.peek() != Token::BracketOpen {
            return Ok(element_type);
        }
        if array_typedef.is_some() {
            return Err(Diagnostic::error(
                "an array of an array-typedef parameter is not supported yet (roadmap)",
            ));
        }

        let mut extents = Vec::new();
        while self.eat_keyword(Token::BracketOpen) {
            let extent = if *self.peek() == Token::BracketClose {
                None
            } else {
                match self.expression()? {
                    Expression::IntegerLiteral(value) if value > 0 => Some(value as u64),
                    _ => None,
                }
            };
            self.expect(Token::BracketClose)?;
            extents.push(extent);
        }

        if extents.len() > 2 {
            return Err(Diagnostic::error(
                "an array parameter with more than two dimensions is not supported yet (roadmap)",
            ));
        }
        if let [_, Some(columns)] = extents.as_slice() {
            let element_bytes = u64::from(element_type.width()) / 8;
            let stride = columns
                .checked_mul(element_bytes)
                .filter(|stride| *stride <= u64::from(u16::MAX))
                .ok_or_else(|| {
                    Diagnostic::error("an array parameter row stride is out of range")
                })?;
            if !name.is_empty() {
                self.decayed_row_pointers
                    .insert(name.to_owned(), (element_type, stride as u16));
            }
        } else if extents.len() == 2 {
            return Err(Diagnostic::error(
                "a non-constant inner array parameter extent is not supported yet (roadmap)",
            ));
        }

        match element_type {
            Type::Struct { size, .. } => Ok(Type::StructPointer { element_size: size }),
            scalar => Ok(Type::Pointer(pointee_of(scalar)?)),
        }
    }
}
