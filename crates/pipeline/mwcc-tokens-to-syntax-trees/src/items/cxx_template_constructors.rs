//! Semantic summaries for constructors of skipped class templates.
//!
//! Template bodies are not parsed by the general C++ frontend yet, but their
//! initializer lists still carry observable lifetime state. The template
//! recovery pass records only initializer facts it can prove; this module maps
//! those facts through a concrete instance layout.

use mwcc_syntax_trees::{Expression, Statement};

use crate::parser::Parser;

/// Materialize scalar zero initializers declared by the primary template's
/// zero-argument constructor at this concrete subobject offset.
pub(crate) fn default_zero_initialization(
    parser: &Parser,
    concrete: &str,
    object_offset: u32,
) -> Option<Vec<Statement>> {
    let primary = concrete
        .split('<')
        .next()?
        .rsplit("::")
        .next()?;
    let template = parser.struct_templates.get(primary)?;
    if template.default_constructor_zero_fields.is_empty() {
        return None;
    }
    let layout = parser.structs.get(concrete)?;
    let statements = template
        .default_constructor_zero_fields
        .iter()
        .filter_map(|name| layout.fields.get(name))
        .map(|field| Statement::Store {
            target: Expression::Member {
                base: Box::new(Expression::Variable("this".to_string())),
                offset: object_offset + field.offset,
                member_type: field.member_type,
                index_stride: None,
            },
            value: Expression::IntegerLiteral(0),
        })
        .collect::<Vec<_>>();
    (!statements.is_empty()).then_some(statements)
}
