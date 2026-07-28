//! Translation-unit packed string storage.
//!
//! `-str pool` gives all literals one `@stringBaseN` object. This module owns
//! byte interning and delegates late instruction scheduling to `addressing`.

mod addressing;
mod schedule;

pub(crate) use addressing::materialize_function_offsets;

#[derive(Default)]
pub(crate) struct PackedStrings {
    offsets: std::collections::HashMap<Vec<u8>, u32>,
    bytes: Vec<u8>,
}

#[derive(Default)]
pub(crate) struct PendingInitializerStrings {
    entries: Vec<PendingInitializerString>,
    global_names: std::collections::HashMap<Vec<u8>, String>,
    next_placeholder: usize,
}

struct PendingInitializerString {
    functions_before: usize,
    phase: u8,
    sequence: usize,
    placeholder: String,
    bytes: Vec<u8>,
}

impl PendingInitializerStrings {
    pub(crate) fn defer_global(&mut self, functions_before: usize, bytes: &[u8]) -> String {
        if let Some(name) = self.global_names.get(bytes) {
            return name.clone();
        }
        let placeholder = self.new_placeholder();
        self.global_names
            .insert(bytes.to_vec(), placeholder.clone());
        self.push(functions_before, 1, placeholder.clone(), bytes);
        placeholder
    }

    pub(crate) fn defer_static_local(
        &mut self,
        functions_before: usize,
        bytes: &[u8],
    ) -> String {
        let placeholder = self.new_placeholder();
        self.push(functions_before, 0, placeholder.clone(), bytes);
        placeholder
    }

    pub(crate) fn materialize(
        mut self,
        pool: &mut PackedStrings,
    ) -> std::collections::HashMap<String, i32> {
        self.entries.sort_by_key(|entry| {
            (entry.functions_before, entry.phase, entry.sequence)
        });
        self.entries
            .into_iter()
            .map(|entry| (entry.placeholder, pool.intern(&entry.bytes) as i32))
            .collect()
    }

    fn new_placeholder(&mut self) -> String {
        let placeholder = format!("@@packed{}", self.next_placeholder);
        self.next_placeholder += 1;
        placeholder
    }

    fn push(&mut self, functions_before: usize, phase: u8, placeholder: String, bytes: &[u8]) {
        let sequence = self.entries.len();
        self.entries.push(PendingInitializerString {
            functions_before,
            phase,
            sequence,
            placeholder,
            bytes: bytes.to_vec(),
        });
    }
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

    #[test]
    fn initializer_strings_follow_source_positions() {
        let mut pending = super::PendingInitializerStrings::default();
        let later_global = pending.defer_global(4, b"later");
        let earlier_global = pending.defer_global(2, b"earlier");
        let local = pending.defer_static_local(4, b"local");
        let mut pool = PackedStrings::default();
        let offsets = pending.materialize(&mut pool);

        assert_eq!(offsets[&earlier_global], 0);
        assert_eq!(offsets[&local], 8);
        assert_eq!(offsets[&later_global], 14);
        assert_eq!(pool.into_bytes(), b"earlier\0local\0later\0");
    }
}
