//! Constant addresses retain typed pointer arithmetic until it becomes an
//! ELF byte addend. Syntax and precedence belong to the ordinary expression
//! parser; this evaluator accepts only relocatable constant results.

use crate::expressions::fold_constant_expression;
use crate::parser::Parser;
use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{BinaryOperator, Expression, PointerElement};

fn displaced(element: PointerElement, bytes: i64) -> Compilation<PointerElement> {
    let (symbol, previous) = match element {
        PointerElement::Symbol(symbol) => (symbol, 0),
        PointerElement::SymbolWithAddend { symbol, addend } => (symbol, addend),
        _ => {
            return Err(Diagnostic::error(
                "constant pointer offsets require named storage",
            ))
        }
    };
    let addend = previous.wrapping_add(bytes as i32);
    Ok(if addend == 0 {
        PointerElement::Symbol(symbol)
    } else {
        PointerElement::SymbolWithAddend { symbol, addend }
    })
}

impl Parser {
    pub(super) fn constant_pointer_element(
        &self,
        expression: &Expression,
    ) -> Compilation<PointerElement> {
        match expression {
            Expression::Variable(name) => Ok(PointerElement::Symbol(
                self.resolve_cxx_initializer_address(name)
                    .unwrap_or_else(|| name.clone()),
            )),
            Expression::StringLiteral(bytes) => Ok(PointerElement::Str(bytes.clone())),
            Expression::AddressOf { operand } => self.constant_lvalue_address(operand),
            Expression::Cast { operand, .. } => self.constant_pointer_element(operand),
            Expression::Binary {
                operator,
                left,
                right,
            } if matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract) => {
                let (pointer, count, subtract) = if let Ok(count) = fold_constant_expression(right)
                {
                    (left.as_ref(), count, *operator == BinaryOperator::Subtract)
                } else if *operator == BinaryOperator::Add {
                    (right.as_ref(), fold_constant_expression(left)?, false)
                } else {
                    return Err(Diagnostic::error(
                        "a static pointer initializer needs a constant offset",
                    ));
                };
                let stride = self.constant_pointer_stride(pointer).ok_or_else(|| {
                    Diagnostic::error("a static pointer initializer needs a known element size")
                })?;
                let bytes = count.wrapping_mul(i64::from(stride));
                displaced(
                    self.constant_pointer_element(pointer)?,
                    if subtract {
                        bytes.wrapping_neg()
                    } else {
                        bytes
                    },
                )
            }
            _ => {
                let value = fold_constant_expression(expression)?;
                Ok(if value == 0 {
                    PointerElement::Null
                } else {
                    PointerElement::Scalar(value)
                })
            }
        }
    }

    fn constant_pointer_stride(&self, expression: &Expression) -> Option<u32> {
        match expression {
            Expression::Binary {
                operator: BinaryOperator::Add | BinaryOperator::Subtract,
                left,
                right,
            } => self
                .constant_pointer_stride(left)
                .or_else(|| self.constant_pointer_stride(right)),
            _ => self.pointed_element_bytes(expression),
        }
    }

    fn constant_lvalue_address(&self, expression: &Expression) -> Compilation<PointerElement> {
        match expression {
            Expression::Variable(_) => self.constant_pointer_element(expression),
            Expression::Dereference { pointer } => self.constant_pointer_element(pointer),
            Expression::Index { base, index } => {
                let stride = self.constant_pointer_stride(base).ok_or_else(|| {
                    Diagnostic::error("a static array address needs a known element size")
                })?;
                displaced(
                    self.constant_pointer_element(base)?,
                    fold_constant_expression(index)?.wrapping_mul(i64::from(stride)),
                )
            }
            Expression::Member {
                base,
                offset,
                index_stride,
                ..
            } => {
                let address = if let (Some(stride), Expression::Index { base, index }) =
                    (index_stride, base.as_ref())
                {
                    displaced(
                        self.constant_pointer_element(base)?,
                        fold_constant_expression(index)?.wrapping_mul(i64::from(*stride)),
                    )?
                } else {
                    self.constant_pointer_element(base)?
                };
                displaced(address, i64::from(*offset))
            }
            _ => Err(Diagnostic::error(
                "this static initializer is not a constant storage address",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_pointer_offsets_preserve_scaling_casts_and_constant_precedence() {
        let source = "char bytes[32]; short halves[16]; int words[8]; double doubles[8];
            char* a=&bytes[3]; short* b=&halves[3]; int* c=&words[3];
            double* d=&doubles[3]; int* e=words+(1<<2)-2; int* f=3+words;
            char* g=(char*)words+3; int* h=(int*)(bytes+3);
            char* i=(char*)&words[2]+1; int* j=&words[5]-2;
            int* k=&words[0]-1; int* l=words+3-3; int* end=&words[8];";
        let unit = crate::parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            true,
            1,
            3,
        )
        .unwrap();
        let addresses: Vec<_> = unit
            .globals
            .iter()
            .filter_map(|global| {
                let element = global.address_initializer.as_ref()?.first()?;
                Some(match element {
                    PointerElement::Symbol(symbol) => (symbol.as_str(), 0),
                    PointerElement::SymbolWithAddend { symbol, addend } => {
                        (symbol.as_str(), *addend)
                    }
                    _ => panic!("expected a named address"),
                })
            })
            .collect();
        assert_eq!(
            addresses,
            [
                ("bytes", 3),
                ("halves", 6),
                ("words", 12),
                ("doubles", 24),
                ("words", 8),
                ("words", 12),
                ("words", 3),
                ("bytes", 3),
                ("words", 9),
                ("words", 12),
                ("words", -4),
                ("words", 0),
                ("words", 32),
            ]
        );
    }

    #[test]
    fn static_local_pointer_offsets_reach_the_relocation_image() {
        let unit = crate::parse_translation_unit(
            mwcc_source_to_tokens::tokenize(
                "int words[8]; int* get() {
                static int* lead=&words[3]; { static int* tail=words+2; } return lead;
            }",
            )
            .unwrap(),
            true,
            true,
            1,
            3,
        )
        .unwrap();
        let addends: Vec<_> = unit.functions[0]
            .locals
            .iter()
            .flat_map(|local| {
                local
                    .data_relocations
                    .iter()
                    .map(|relocation| relocation.addend)
            })
            .collect();
        assert_eq!(addends, [12, 8]);
    }

    #[test]
    fn runtime_offsets_do_not_become_constant_addresses() {
        let result = crate::parse_translation_unit(
            mwcc_source_to_tokens::tokenize("int words[8]; int index; int* p=words+index;")
                .unwrap(),
            true,
            true,
            1,
            3,
        );
        assert!(result.is_err());
    }
}
