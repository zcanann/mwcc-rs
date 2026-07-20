use crate::FunctionObject;

/// Final placement of function bodies within the translation unit's code
/// section. Symbol and debug emitters consume the same result so deferred
/// materialization and alignment cannot drift between them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionLayout {
    pub order: Vec<usize>,
    pub offsets: Vec<u32>,
    pub sizes: Vec<u32>,
    pub byte_len: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FunctionPlacement {
    pub byte_size: u32,
    pub deferred: bool,
}

pub fn layout_functions(functions: &[FunctionObject<'_>], alignment: u32) -> FunctionLayout {
    let placements: Vec<FunctionPlacement> = functions
        .iter()
        .map(|function| FunctionPlacement {
            byte_size: function.text.len() as u32,
            deferred: function.text_deferred,
        })
        .collect();
    layout_function_placements(&placements, alignment)
}

pub fn layout_function_placements(
    functions: &[FunctionPlacement],
    alignment: u32,
) -> FunctionLayout {
    assert!(alignment.is_power_of_two());
    let mut order = Vec::with_capacity(functions.len());
    let mut pending = Vec::new();
    for (index, function) in functions.iter().enumerate() {
        if function.deferred {
            pending.push(index);
        } else {
            order.push(index);
            order.append(&mut pending);
        }
    }
    order.append(&mut pending);

    let mut offsets = vec![0; functions.len()];
    let mut sizes = vec![0; functions.len()];
    let mut byte_len = 0u32;
    for &index in &order {
        byte_len = byte_len.div_ceil(alignment) * alignment;
        offsets[index] = byte_len;
        sizes[index] = functions[index].byte_size;
        byte_len += sizes[index];
    }
    FunctionLayout {
        order,
        offsets,
        sizes,
        byte_len,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The writer owns the large FunctionObject vocabulary. This narrowly tests
    // layout through a local constructor so deferred ordering has one pin.
    fn function<'a>(name: &'a str, text: &'a [u8], deferred: bool) -> FunctionObject<'a> {
        FunctionObject {
            name,
            is_static: false,
            static_locals_lead: false,
            text_deferred: deferred,
            is_weak: false,
            section: None,
            is_asm: false,
            entry_points: Vec::new(),
            force_active: false,
            text,
            relocations: Vec::new(),
            constants: Vec::new(),
            frame: None,
            anonymous_bump: 0,
            implicit_local: false,
            weak_inline: false,
            constant_number_gaps: Vec::new(),
            phantom_externals: Vec::new(),
            post_constant_bump: 0,
            post_function_anonymous_bump: None,
            string_count: 0,
            string_number_after_constants: None,
            string_number_after_rodata: None,
            string_names: Vec::new(),
            jump_tables: Vec::new(),
            anonymous_rodata: Vec::new(),
            local_undefined_callees: Vec::new(),
            symbol_order: Vec::new(),
            referenced_function_symbols: Vec::new(),
            implicit_external_callees: Vec::new(),
            early_implicit_external_callees: Vec::new(),
        }
    }

    #[test]
    fn deferred_body_follows_the_next_ordinary_body() {
        let functions = [
            function("deferred", &[1, 2, 3, 4], true),
            function("ordinary", &[5, 6, 7, 8], false),
        ];
        let layout = layout_functions(&functions, 16);
        assert_eq!(layout.order, [1, 0]);
        assert_eq!(layout.offsets, [16, 0]);
        assert_eq!(layout.byte_len, 20);
    }
}
