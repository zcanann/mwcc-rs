//! Source identities for enumeration declarations.
//!
//! Executable lowering deliberately represents an enum by its configured
//! integer storage type.  Debug lowering still needs the declaration name,
//! enumerator order, and written values, so retain those facts in a parallel
//! translation-unit graph instead of widening every executable expression.

/// One enum declaration retained in source order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumerationDefinition {
    /// Parser identity used by typedefs and function type side tables.
    pub name: String,
    /// Name written into debug information. Anonymous enums acquire the first
    /// typedef alias that names them; a truly anonymous enum remains `None`.
    pub source_name: Option<String>,
    pub byte_size: u8,
    pub enumerators: Vec<Enumerator>,
}

/// One enumerator in declaration order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enumerator {
    pub name: String,
    pub value: i64,
}
