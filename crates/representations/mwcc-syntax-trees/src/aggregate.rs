//! Source identities for aggregate declarations.
//!
//! Executable lowering only needs resolved sizes and member offsets, so the
//! compact [`crate::Type`] representation intentionally erases struct tags and
//! member declarations.  Debug lowering needs those source facts.  Keep them
//! in a parallel, representation-owned graph instead of teaching machine IR
//! about C declarations or reconstructing types from emitted data.

use crate::Type;

/// One named struct or union declaration retained by the translation unit.
#[derive(Debug, Clone, PartialEq)]
pub struct AggregateDefinition {
    pub name: String,
    pub byte_size: u32,
    pub alignment: u8,
    pub is_union: bool,
    pub members: Vec<AggregateMember>,
}

/// One aggregate member in source declaration order.
#[derive(Debug, Clone, PartialEq)]
pub struct AggregateMember {
    pub name: String,
    pub declared_type: Type,
    pub offset: u32,
    /// The named aggregate behind a struct value or pointer member.
    pub aggregate_tag: Option<String>,
    /// Total element count for an array member. `None` denotes a scalar.
    pub array_length: Option<u32>,
    /// `(bit offset from the most-significant end, width)` for a bit-field.
    pub bit_field: Option<(u8, u8)>,
}
