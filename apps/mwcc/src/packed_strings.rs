//! Translation-unit packed string storage.
//!
//! `-str pool` gives all literals one `@stringBaseN` object. This module owns
//! byte interning and offsets; the driver remains responsible for source-order
//! scheduling and relocation rewriting.

#[derive(Default)]
pub(crate) struct PackedStrings {
    offsets: std::collections::HashMap<Vec<u8>, u32>,
    bytes: Vec<u8>,
}

impl PackedStrings {
    pub(crate) fn intern(&mut self, literal: &[u8]) -> u32 {
        if let Some(offset) = self.offsets.get(literal) {
            return *offset;
        }
        let offset = self.bytes.len() as u32;
        self.bytes.extend_from_slice(literal);
        self.bytes.push(0);
        self.offsets.insert(literal.to_vec(), offset);
        offset
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub(crate) fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::PackedStrings;

    #[test]
    fn interns_literals_without_per_literal_padding() {
        let mut pool = PackedStrings::default();
        assert_eq!(pool.intern(b"%d"), 0);
        assert_eq!(pool.intern(b"%c"), 3);
        assert_eq!(pool.intern(b"%d"), 0);
        assert_eq!(pool.into_bytes(), b"%d\0%c\0");
    }
}
