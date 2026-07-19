//! Metrowerks C++ surface syntax kept out of the general C item parser.
//!
//! Linkage specifications are declaration wrappers, not declarations themselves;
//! normalization removes those wrappers before recursive descent. Symbol names
//! use CodeWarrior's own mangling rather than the Itanium ABI.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{Pointee, Type};
use mwcc_tokens::Token;

/// Remove C++ linkage-specification syntax while retaining every enclosed token
/// in source order. `extern "C" { declarations }` becomes `declarations`, and
/// `extern "C" declaration` keeps the `extern` storage class but drops the
/// language string. The latter distinction matters for data declarations.
pub(crate) fn normalize_linkage_specifications(mut tokens: Vec<Token>) -> Vec<Token> {
    let mut index = 0usize;
    while index + 1 < tokens.len() {
        let starts_linkage = matches!(&tokens[index], Token::Identifier(word) if word == "extern")
            && matches!(&tokens[index + 1], Token::StringLiteral(language) if language == b"C" || language == b"C++");
        if !starts_linkage {
            index += 1;
            continue;
        }

        if tokens.get(index + 2) == Some(&Token::BraceOpen) {
            let mut cursor = index + 2;
            let mut depth = 0i32;
            let mut close = None;
            while cursor < tokens.len() {
                match tokens[cursor] {
                    Token::BraceOpen => depth += 1,
                    Token::BraceClose => {
                        depth -= 1;
                        if depth == 0 {
                            close = Some(cursor);
                            break;
                        }
                    }
                    Token::EndOfFile => break,
                    _ => {}
                }
                cursor += 1;
            }
            if let Some(close) = close {
                tokens.remove(close);
                tokens.drain(index..index + 3);
                continue;
            }
        } else {
            // Keep `extern` so an object declaration remains a declaration rather
            // than becoming a tentative definition.
            tokens.remove(index + 1);
        }
        index += 1;
    }
    tokens
}

/// Mangle an ordinary, singly-qualified member function.
///
/// Examples measured from mwcceppc:
/// `void KartCannon::Init(int)` -> `Init__10KartCannonFi`
/// `void KartCannon::DoKeep()` -> `DoKeep__10KartCannonFv`
pub(crate) fn mangle_member_function(
    scope: &str,
    function: &str,
    explicit_parameters: &[Type],
) -> Compilation<String> {
    if scope.is_empty() || function.is_empty() {
        return Err(Diagnostic::error("an empty C++ member name is invalid"));
    }
    let arguments = if explicit_parameters.is_empty() {
        "v".to_string()
    } else {
        explicit_parameters
            .iter()
            .copied()
            .map(encode_type)
            .collect::<Compilation<Vec<_>>>()?
            .concat()
    };
    Ok(format!("{function}__{}{scope}F{arguments}", scope.len()))
}

fn encode_type(parameter: Type) -> Compilation<String> {
    let code = match parameter {
        Type::Int => "i".to_string(),
        Type::UnsignedInt => "Ui".to_string(),
        Type::Char => "c".to_string(),
        Type::UnsignedChar => "Uc".to_string(),
        Type::Short => "s".to_string(),
        Type::UnsignedShort => "Us".to_string(),
        Type::Float => "f".to_string(),
        Type::Double => "d".to_string(),
        Type::LongLong => "x".to_string(),
        Type::UnsignedLongLong => "Ux".to_string(),
        Type::Pointer(pointee) => format!("P{}", encode_pointee(pointee)?),
        Type::Void => {
            return Err(Diagnostic::error(
                "a named void C++ parameter is not supported",
            ))
        }
        Type::StructPointer { .. } | Type::Struct { .. } => {
            return Err(Diagnostic::error(
                "a struct-valued C++ member parameter needs qualified type mangling (roadmap)",
            ))
        }
    };
    Ok(code)
}

fn encode_pointee(pointee: Pointee) -> Compilation<&'static str> {
    match pointee {
        Pointee::Int => Ok("i"),
        Pointee::UnsignedInt => Ok("Ui"),
        Pointee::Char => Ok("c"),
        Pointee::UnsignedChar => Ok("Uc"),
        Pointee::Short => Ok("s"),
        Pointee::UnsignedShort => Ok("Us"),
        Pointee::Float => Ok("f"),
        Pointee::Double => Ok("d"),
        Pointee::LongLong => Ok("x"),
        Pointee::UnsignedLongLong => Ok("Ux"),
        Pointee::Pointer | Pointee::WordPointer => Err(Diagnostic::error(
            "a pointer-to-pointer C++ member parameter needs exact pointee mangling (roadmap)",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_block_linkage_without_losing_declarations() {
        let tokens = vec![
            Token::Identifier("extern".to_string()),
            Token::StringLiteral(b"C".to_vec()),
            Token::BraceOpen,
            Token::KeywordInt,
            Token::Identifier("value".to_string()),
            Token::Semicolon,
            Token::BraceClose,
            Token::EndOfFile,
        ];
        assert_eq!(
            normalize_linkage_specifications(tokens),
            vec![
                Token::KeywordInt,
                Token::Identifier("value".to_string()),
                Token::Semicolon,
                Token::EndOfFile,
            ]
        );
    }

    #[test]
    fn mangles_measured_member_shapes() {
        assert_eq!(
            mangle_member_function("KartCannon", "Init", &[Type::Int]).unwrap(),
            "Init__10KartCannonFi"
        );
        assert_eq!(
            mangle_member_function("KartCannon", "DoKeep", &[]).unwrap(),
            "DoKeep__10KartCannonFv"
        );
    }
}
