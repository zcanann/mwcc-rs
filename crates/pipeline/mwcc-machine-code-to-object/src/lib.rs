//! Pipeline: machine code -> relocatable object file.
//!
//! Encodes the function's instructions to `.text` and wraps them in an ELF32
//! big-endian PowerPC object matching mwcceppc's layout (sections, symbols,
//! relocations, and the Metrowerks metadata records).

use mwcc_machine_code::{
    MachineFunction, RelocationKind as MachineRelocationKind, RelocationTarget as MachineTarget,
};
use mwcc_object::{
    DataObject, FrameLayout, FunctionObject, JumpTable, ObjectInput, RelocationTarget,
    Sdata2Constant, TextRelocation,
};

pub use mwcc_object::DebugSections;
/// A data-section `ADDR32` relocation (re-exported so callers can build a
/// `DefinedGlobal`'s relocations without depending on `mwcc-object` directly).
pub use mwcc_object::{CommentFormat, DataRelocation, FunctionSymbolOrder, ObjectFormat};

fn constant_uses_absolute_addressing(function: &MachineFunction, constant_index: usize) -> bool {
    function.relocations.iter().any(|relocation| {
        let targets_constant = matches!(
            relocation.target,
            MachineTarget::Constant(index) | MachineTarget::ConstantWithAddend(index, _)
                if index == constant_index
        );
        targets_constant
            && matches!(
                relocation.kind,
                MachineRelocationKind::Addr16Ha
                    | MachineRelocationKind::Addr16Hi
                    | MachineRelocationKind::Addr16Lo
            )
    })
}

/// A file-scope variable *defined* in this unit (placed in a data section), in
/// declaration order. The caller decides which globals qualify (non-`extern`,
/// laid out); the object writer assigns their section offsets and symbols.
pub struct DefinedGlobal {
    pub name: String,
    pub size: u32,
    /// Alignment used to lay out the object in its data section.
    pub alignment: u32,
    /// Alignment recorded for the symbol in CodeWarrior's `.comment` metadata.
    /// This can be smaller than the storage alignment: scalar arrays are laid
    /// out at a word boundary, but retain their element alignment here.
    pub comment_alignment: u32,
    pub initial_bytes: Option<Vec<u8>>,
    /// A `const` global is read-only: the writer routes it to `.sdata2` (≤ 8
    /// bytes) or `.rodata` (larger) rather than `.sdata`/`.sbss`.
    pub is_const: bool,
    /// Bypass the small-data threshold for this object. The syntax lowering sets
    /// this for generation-specific source cases (currently inferred arrays),
    /// leaving the writer independent of C declarator syntax.
    pub force_full_data_section: bool,
    /// A `static` global binds as a LOCAL symbol (file-scope, not exported).
    pub is_static: bool,
    /// An EXPLICITLY zero-initialized global (`int a = 0;`) rather than an
    /// uninitialized one (`int a;`). Both land in `.sbss`/`.bss`, but the writer lays
    /// the explicit-zero ones in declaration order ahead of the reversed uninitialized run.
    pub is_explicit_zero: bool,
    /// A compiler-created data temporary whose `@N` identity was assigned by
    /// frontend/optimizer analysis before ordinary emitted functions. Unlike a
    /// pooled literal, it does not merely consume one slot from the writer's
    /// dense front-of-unit counter: the measured ordinal can be sparse and
    /// therefore also establishes a floor for later anonymous objects.
    pub preassigned_anonymous_ordinal: Option<u32>,
    /// `ADDR32` data relocations the global's bytes carry (a pointer to a symbol).
    pub relocations: Vec<mwcc_object::DataRelocation>,
    /// Non-static functions defined before this object (source-order symbol interleaving).
    pub non_static_functions_before: usize,
    pub functions_before: usize,
    /// A WEAK object symbol (an inline function's emitted static local).
    pub is_weak: bool,
    /// A real function's STATIC LOCAL: the owning function's index. The writer
    /// numbers it off that function's @N sequence and displays `name$K`.
    pub static_local_owner: Option<usize>,
    /// Signed shift a static local's `$N` takes off the owner's base counter —
    /// the declaration-position part of the unit's inline pre-bump.
    pub anonymous_adjust: i64,
    /// An explicit `__declspec(section "…")` output section (e.g. `.dtors`),
    /// overriding the default routing. `None` uses the size/const/zero rules.
    pub section: Option<String>,
}

/// Assemble a relocatable object from one or more lowered functions (in source
/// order) plus the file-scope variables defined in the unit. `source_name` is the
/// source file's base name (e.g. "foo.c"), used for the object's `FILE` symbol;
/// `version` is the compiler version being reproduced, stamped into `.comment`.
/// The functions may select distinct code sections and share the constant pool,
/// unwind sections, and data sections.
pub fn assemble_object(
    functions: &[MachineFunction],
    defined_globals: &[DefinedGlobal],
    inline_asm_symbols: &[String],
    forward_declared_statics: &[String],
    early_undefined_externals: &[String],
    section_function_declarations: &[String],
    section_externals: &[(String, usize)],
    source_name: &str,
    object_format: ObjectFormat,
    small_data: bool,
    emit_mwcats: bool,
    debug: Option<DebugSections>,
) -> Vec<u8> {
    // The encoded text is owned here so the borrowed `FunctionObject` can point at
    // it for the lifetime of the call.
    let texts: Vec<Vec<u8>> = functions
        .iter()
        .map(|function| function.encode_text())
        .collect();
    let function_objects = functions
        .iter()
        .zip(&texts)
        .map(|(function, text)| FunctionObject {
            name: &function.name,
            is_static: function.is_static,
            static_locals_lead: function.static_locals_lead,
            text_deferred: function.text_deferred,
            implicit_local: function.implicit_materialized,
            weak_inline: function.weak_inline,
            is_weak: function.is_weak,
            section: function.section.as_deref(),
            is_asm: function.is_asm,
            entry_points: function
                .entry_points
                .iter()
                .map(|(name, index)| (name.clone(), *index as u32 * 4))
                .collect(),
            force_active: function.force_active,
            text,
            // Each codegen relocation patches one instruction; its byte offset
            // (relative to the function) is four times the instruction index plus
            // the kind's field offset (the ADDR16 immediate sits in the low
            // halfword, at instruction+2).
            relocations: function
                .relocations
                .iter()
                .map(|relocation| TextRelocation {
                    offset: relocation.instruction_index as u32 * 4
                        + if relocation.kind == mwcc_machine_code::RelocationKind::EmbSda21 {
                            u32::from(object_format.emb_sda21_offset)
                        } else {
                            relocation.kind.field_offset()
                        },
                    elf_type: relocation.kind.elf_type(),
                    target: match &relocation.target {
                        MachineTarget::External(symbol) => {
                            RelocationTarget::External(symbol.clone())
                        }
                        MachineTarget::ExternalWithAddend(symbol, addend) => {
                            RelocationTarget::ExternalWithAddend(symbol.clone(), *addend)
                        }
                        MachineTarget::Constant(index) => RelocationTarget::Constant(*index),
                        MachineTarget::ConstantWithAddend(index, addend) => {
                            RelocationTarget::ConstantWithAddend(*index, *addend)
                        }
                        MachineTarget::JumpTable => RelocationTarget::JumpTable,
                        MachineTarget::JumpTableAt(table_index) => {
                            RelocationTarget::JumpTableAt(*table_index)
                        }
                        MachineTarget::AnonymousRodata => RelocationTarget::AnonymousRodata,
                        MachineTarget::AnonymousRodataAt(blob_index) => {
                            RelocationTarget::AnonymousRodataAt(*blob_index)
                        }
                    },
                })
                .collect(),
            constants: function
                .constants
                .iter()
                .enumerate()
                .map(|(constant_index, constant)| Sdata2Constant {
                    bits: constant.bits,
                    byte_width: constant.byte_width,
                    static_slot: constant.static_slot,
                    image: constant.image,
                    force_new: constant.force_new,
                    force_full_data_section: constant_uses_absolute_addressing(
                        function,
                        constant_index,
                    ),
                })
                .collect(),
            frame: function.frame.map(|frame| FrameLayout {
                extab_header: frame.extab_header(),
            }),
            // The anonymous-`@N` counter is bumped by one for an int<->float
            // conversion and by three for a float conditional branch before this
            // function's constants are numbered.
            anonymous_bump: function.object_anonymous_bump(),
            post_constant_bump: function.post_constant_label_bump,
            post_function_anonymous_bump: function.post_function_anonymous_bump,
            constant_number_gaps: function.constant_number_gaps.clone(),
            constant_number_adjust: function.constant_number_adjust,
            phantom_externals: function.phantom_externals.clone(),
            // The unit's string resolver set these: the function's NEW-string count and the `@N`
            // names of those strings. The writer numbers/emits them at the front of the function's
            // `@N` block (interleaved per-function with its constants and unwind entries).
            string_count: function.new_string_count,
            string_number_after_constants: function.string_number_after_constants,
            string_number_after_rodata: function.string_number_after_rodata,
            string_names: function.new_string_names.clone(),
            jump_tables: function
                .jump_tables
                .iter()
                .map(|table| JumpTable {
                    entries: table.entries.clone(),
                    anonymous_offset: table.anonymous_offset,
                })
                .collect(),
            anonymous_rodata: function
                .anonymous_rodata
                .iter()
                .map(|blob| (blob.bytes.clone(), blob.anonymous_offset))
                .collect(),
            local_undefined_callees: function.local_undefined_callees.clone(),
            symbol_order: function.symbol_order.clone(),
            referenced_function_symbols: function.referenced_function_symbols.clone(),
            implicit_external_callees: function.implicit_external_callees.clone(),
            early_implicit_external_callees: function.early_implicit_external_callees.clone(),
        })
        .collect();
    let data_objects = defined_globals
        .iter()
        .map(|global| DataObject {
            name: &global.name,
            size: global.size,
            alignment: global.alignment,
            comment_alignment: global.comment_alignment,
            initial_bytes: global.initial_bytes.clone(),
            is_const: global.is_const,
            force_full_data_section: global.force_full_data_section,
            is_static: global.is_static,
            is_explicit_zero: global.is_explicit_zero,
            preassigned_anonymous_ordinal: global.preassigned_anonymous_ordinal,
            relocations: global.relocations.clone(),
            non_static_functions_before: global.non_static_functions_before,
            functions_before: global.functions_before,
            is_weak: global.is_weak,
            static_local_owner: global.static_local_owner,
            anonymous_adjust: global.anonymous_adjust,
            section: global.section.as_deref(),
        })
        .collect();
    let local_symbol_order = functions
        .iter()
        .find(|function| !function.local_symbol_order.is_empty())
        .map_or(&[][..], |function| function.local_symbol_order.as_slice());
    mwcc_object::write_object(&ObjectInput {
        source_name,
        object_format,
        functions: function_objects,
        data_objects,
        small_data,
        emit_mwcats,
        inline_asm_symbols,
        forward_declared_statics,
        early_undefined_externals,
        section_function_declarations,
        section_externals,
        local_symbol_order,
        debug,
    })
}
