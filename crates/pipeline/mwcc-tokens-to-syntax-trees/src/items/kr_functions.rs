//! Old-style (K&R) C function parameter declarations.
//!
//! In a definition such as `int f(a, b) int a; short b; { ... }`, the
//! identifier list establishes parameter order while the declarations between
//! `)` and `{` supply their types. Keeping that reconciliation here avoids
//! adding a second parameter grammar to the already-large top-level item
//! parser.

use std::collections::{HashMap, HashSet};

use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{Parameter, Type};
use mwcc_tokens::Token;

use crate::parser::Parser;

use super::pointee_of;

impl Parser {
    /// Parse an old-style identifier list and its following declarations.
    ///
    /// The cursor starts immediately after the function declarator's `(` and
    /// finishes at the body-opening `{`. Parameters omitted from the declaration
    /// list have C89's implicit `int` type.
    pub(super) fn parse_kr_parameters(&mut self) -> Compilation<Vec<Parameter>> {
        let mut names = Vec::new();
        let mut name_indexes = HashMap::new();

        loop {
            let name_position = self.position;
            let name = self.parse_identifier()?;
            if name_indexes.insert(name.clone(), names.len()).is_some() {
                return Err(Diagnostic::error(format!(
                    "duplicate old-style parameter '{name}'"
                )));
            }
            self.record_named_parameter_at(name_position);
            names.push(name);
            if *self.peek() != Token::Comma {
                break;
            }
            self.advance();
        }
        self.expect(Token::ParenClose)?;

        let mut resolved = vec![None; names.len()];
        let mut declared = HashSet::new();
        while *self.peek() != Token::BraceOpen {
            if *self.peek() == Token::EndOfFile {
                return Err(Diagnostic::error(
                    "unterminated old-style parameter declarations",
                ));
            }

            let declaration_type = self.parse_type()?;
            let declaration_struct_tag = self.last_struct_tag.take();
            let array_typedef = self.last_array_typedef.take();
            let mut first_declarator = true;

            loop {
                let mut parameter_type = declaration_type;
                if !first_declarator && *self.peek() == Token::Star {
                    // `T *a, *b`: parse_type consumed the first declarator's
                    // star. A matching star on a later declarator denotes the
                    // same pointer type rather than another pointer level.
                    self.advance();
                    if *self.peek() == Token::Star {
                        return Err(Diagnostic::error(
                            "a multi-level old-style parameter declarator list is not supported yet (roadmap)",
                        ));
                    }
                    if !matches!(
                        parameter_type,
                        Type::Pointer(_) | Type::StructPointer { .. }
                    ) {
                        parameter_type = Type::Pointer(pointee_of(parameter_type)?);
                    }
                } else if !first_declarator
                    && matches!(
                        parameter_type,
                        Type::Pointer(_) | Type::StructPointer { .. }
                    )
                {
                    return Err(Diagnostic::error(
                        "a mixed pointer/non-pointer old-style parameter declarator list is not supported yet (roadmap)",
                    ));
                }

                let name = self.parse_identifier()?;
                let Some(&index) = name_indexes.get(&name) else {
                    return Err(Diagnostic::error(format!(
                        "old-style declaration names non-parameter '{name}'"
                    )));
                };
                if !declared.insert(name.clone()) {
                    return Err(Diagnostic::error(format!(
                        "duplicate declaration for old-style parameter '{name}'"
                    )));
                }

                if *self.peek() == Token::BracketOpen {
                    if array_typedef.is_some() {
                        return Err(Diagnostic::error(
                            "an array of an array-typedef old-style parameter is not supported yet (roadmap)",
                        ));
                    }
                    self.advance();
                    while !matches!(self.peek(), Token::BracketClose | Token::EndOfFile) {
                        self.advance();
                    }
                    self.expect(Token::BracketClose)?;
                    parameter_type = match parameter_type {
                        Type::Struct { size, .. } => Type::StructPointer {
                            element_size: size,
                        },
                        scalar => Type::Pointer(pointee_of(scalar)?),
                    };
                }

                if let Some(tag) = &declaration_struct_tag {
                    self.variable_structs.insert(name.clone(), tag.clone());
                } else {
                    self.variable_structs.remove(&name);
                }
                if let Some((element, _total, inner)) = array_typedef {
                    let stride = inner.max(1) as u32 * (element.width() as u32 / 8);
                    self.decayed_row_pointers
                        .insert(name.clone(), (element, stride as u16));
                }
                resolved[index] = Some(parameter_type);

                if *self.peek() != Token::Comma {
                    break;
                }
                self.advance();
                first_declarator = false;
            }
            self.expect(Token::Semicolon)?;
        }

        Ok(names
            .into_iter()
            .enumerate()
            .map(|(index, name)| {
                if resolved[index].is_none() {
                    // Parser state is shared across function bodies. An
                    // implicit-int parameter must not inherit an aggregate
                    // identity from a same-named parameter parsed earlier.
                    self.variable_structs.remove(&name);
                }
                Parameter {
                    parameter_type: resolved[index].unwrap_or(Type::Int),
                    name,
                }
            })
            .collect())
    }
}
