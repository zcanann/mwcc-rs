//! Shared data routing and BSS placement for code finalization and ELF writing.

use crate::{DataObject, ObjectFormat};
use std::collections::{HashMap, HashSet};

pub fn data_section(object: &DataObject<'_>, small_data: bool) -> &'static str {
    // An explicit `__declspec(section "…")` override wins over the default
    // routing. Only the sections the writer knows how to emit are honored.
    if let Some(section) = object.section {
        return match section {
            ".ctors" => ".ctors",
            ".dtors" => ".dtors",
            ".sdata" => ".sdata",
            ".sbss" => ".sbss",
            ".sdata2" => ".sdata2",
            ".sbss2" => ".sbss2",
            ".rodata" => ".rodata",
            ".bss" => ".bss",
            _ => ".data",
        };
    }
    if object.is_const {
        if object.size <= 8 && !object.force_full_data_section {
            ".sdata2"
        } else {
            ".rodata"
        }
    } else if !small_data {
        // `-sdata 0` disables the small writable sections entirely. Small
        // and large definitions share one `.data`/`.bss` layout; merely
        // renaming separate `.sdata` and `.data` sections would create two
        // same-named ELF sections with independent offsets.
        if object.initial_bytes.is_some() {
            ".data"
        } else {
            ".bss"
        }
    } else if object.size <= 8 && !object.force_full_data_section {
        if object.initial_bytes.is_some() {
            ".sdata"
        } else {
            ".sbss"
        }
    } else if object.initial_bytes.is_some() {
        ".data"
    } else {
        ".bss"
    }
}

/// Return input indices in physical BSS order. References are per-function
/// source symbols, relocation targets, and late displacements, in that order.
pub fn bss_object_order(
    objects: &[DataObject<'_>],
    references: &[Vec<String>],
    small_data: bool,
    format: ObjectFormat,
) -> Vec<usize> {
    let bss: Vec<_> = objects
        .iter()
        .enumerate()
        .filter(|(_, object)| data_section(object, small_data) == ".bss")
        .map(|(index, _)| index)
        .collect();
    let mut order = Vec::new();
    let mut seen = HashSet::new();
    let mut place = |index: usize| {
        if seen.insert(objects[index].name) {
            order.push(index);
        }
    };
    if format.zero_data_in_declaration_order {
        let mut declarations = bss.clone();
        declarations.sort_by_key(|index| {
            let object = &objects[*index];
            (
                object
                    .static_local_owner
                    .unwrap_or(object.functions_before)
                    .min(references.len()),
                object.static_local_owner.is_some(),
            )
        });
        for index in declarations {
            place(index);
        }
    }
    if !small_data || format.local_data_symbols_in_declaration_order {
        for index in &bss {
            if objects[*index].is_static {
                place(*index);
            }
        }
    }
    if !small_data {
        for index in bss.iter().rev() {
            if !objects[*index].is_static {
                place(*index);
            }
        }
    } else {
        let mut names = HashMap::new();
        for index in &bss {
            names.entry(objects[*index].name).or_insert(*index);
        }
        for name in references.iter().flatten() {
            if let Some(index) = names.get(name.as_str()) {
                place(*index);
            }
        }
        for index in bss.iter().rev() {
            place(*index);
        }
    }
    order
}
