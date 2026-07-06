//! A function's worth of machine code, and its lowering to `.text` bytes.

use crate::frame::FrameInfo;
use crate::instruction::Instruction;
use crate::relocation::Relocation;

/// A read-only constant in the `.sdata2` pool: its big-endian bit pattern and
/// byte width (4 for a single-precision float, 8 for the int->float bias double).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolConstant {
    pub bits: u64,
    pub byte_width: u8,
    /// Numbered at the function's STATIC-LOCAL slot (`counter - 1`) instead of
    /// the pool block — mwcc pools an initialized AUTO array's word image there
    /// (measured: mbstring's first_byte_mark -> @4).
    pub static_slot: bool,
    /// An initialized AUTO array's pooled word image: its `.sdata2` SYMBOL
    /// leads the owning static function's FUNC symbol regardless of where it
    /// NUMBERS (mp4 @4 static-slot; ww @47 pool-block — both lead).
    pub image: bool,
}

/// An anonymous `.rodata` blob (`@N`): raw bytes the writer materializes as a
/// LOCAL object, addressed by the function via `R_PPC_ADDR16_HA`/`_LO`.
#[derive(Debug, Clone)]
pub struct AnonymousRodata {
    pub bytes: Vec<u8>,
    /// How far past the function's running anonymous-`@N` counter the blob's
    /// number sits (measured; signed — __strtold's table lands at counter-1,
    /// like a static local).
    pub anonymous_offset: i32,
}

/// A dense `switch`'s jump table (the writer materializes it as an anonymous `@N`
/// object in `.data`).
#[derive(Debug, Clone)]
pub struct JumpTable {
    /// One `.text` byte offset (within the function) per index — the body the
    /// dispatch branches to (gaps point at the default body).
    pub entries: Vec<u32>,
    /// How far past the function's running anonymous-`@N` counter the table's
    /// symbol sits: a label per case plus the dispatch, and one more for an
    /// explicit `default:` label.
    pub anonymous_offset: u32,
}

/// A function's worth of machine code.
#[derive(Debug, Clone, Default)]
pub struct MachineFunction {
    pub name: String,
    /// An explicit `__declspec(section "…")` code section (e.g. `.init`), overriding
    /// the default `.text` placement. `None` = `.text`.
    pub section: Option<String>,
    pub instructions: Vec<Instruction>,
    /// `.text` relocations, by the instruction they patch.
    pub relocations: Vec<Relocation>,
    /// Read-only constants this function loads from `.sdata2`. Each becomes an
    /// anonymous `@N` object that the function's `R_PPC_EMB_SDA21` loads reference.
    pub constants: Vec<PoolConstant>,
    /// String literals used in this function's body, by bytes (without the trailing
    /// NUL), in first-use order. The unit resolver pools these into anonymous `@N`
    /// `.sdata` objects and rewrites the placeholder `@@strN` relocations.
    pub string_literals: Vec<Vec<u8>>,
    /// The count of NEW (non-reused) strings this function contributes to the unit's
    /// anonymous-`@N` pool — set by the unit's string resolver. The object writer
    /// advances its per-function `@N` counter by this at the FRONT of the function's
    /// block (strings precede the function's constants and unwind entries).
    pub new_string_count: u32,
    /// The `@N` names of the NEW strings this function introduces, in front-of-block
    /// order — set by the unit's string resolver alongside `new_string_count`. The
    /// object writer emits a LOCAL symbol for each at the FRONT of the function's `@N`
    /// block (before its constants and unwind entries), so a second string-bearing
    /// function interleaves its string symbol per-function the way mwcc does.
    pub new_string_names: Vec<String>,
    /// A `static` (file-local) function — emitted with a LOCAL `STT_FUNC` symbol
    /// rather than a global one.
    pub is_static: bool,
    /// A static function MATERIALIZED from an implicitly-declared inline: its
    /// call relocations bind the UND ghost and its local symbol trails its
    /// static locals (ww uart).
    pub implicit_materialized: bool,
    /// A WEAK function materialized from a PLAIN inline: its .comment flag is
    /// the weak-OBJECT 0x0d, not declspec-weak's 0x0e (strikers mbstowcs).
    pub weak_inline: bool,
    pub is_weak: bool,
    /// The function's STATIC LOCALS: (name, byte image or None for zero,
    /// byte size, alignment, is_const). Emitted as `name$K` LOCAL objects,
    /// K taken from the function's @N sequence front.
    pub static_locals: Vec<(String, Option<Vec<u8>>, u32, u32, bool)>,
    /// Whether the function performs an int<->float conversion. mwcc's anonymous
    /// `@N` counter starts one higher for such functions.
    pub has_conversion: bool,
    /// Mid-pool `@N` gaps: (constant index, extra numbers consumed BEFORE that
    /// constant is numbered) — an int<->float conversion's internal label sits
    /// between the constants it separates (k_tan's @69 -> @71 jump).
    pub constant_number_gaps: Vec<(usize, u32)>,
    /// Named `static const` SCALARS this function's TU must EMIT (mwcc
    /// usually folds/elides them, but some header/source contexts keep the
    /// named .sdata2 object — measured per capture; ww's e_pow keeps `one`).
    pub keep_named_const_scalars: Vec<String>,
    /// UND externals with NO relocation — symbols mwcc created while compiling
    /// a later-dropped inline body (strikers' __frsqrte). Emitted FIRST among
    /// this function's externals.
    pub phantom_externals: Vec<String>,
    /// Whether the function emits a floating-point conditional branch. mwcc's
    /// anonymous `@N` counter advances by three for such a branch.
    pub has_float_branch: bool,
    /// Extra `@N`-counter advance from the function's internal control-flow labels:
    /// mwcc numbers the basic-block labels an `if`/loop introduces before the
    /// function's unwind `@N` entries. Measured per construct — a non-leaf `if` adds
    /// 2, a `do … while` loop adds 6.
    pub anonymous_label_bump: u32,
    /// Extra `@N` numbers consumed AFTER this function's pooled constants
    /// and before its extab pair (the nested punned-guard's inner block).
    pub post_constant_label_bump: u32,
    /// Emitted by the DAG emitter: the order IS the schedule — the legacy
    /// post-allocation scheduling passes must not touch it.
    pub pre_scheduled: bool,
    /// Frame metadata for the unwind tables; `None` for a leaf with no frame.
    pub frame: Option<FrameInfo>,
    /// A dense `switch`'s jump table; `None` unless the function dispatches through
    /// one. The writer materializes it as an anonymous `@N` object in `.data`.
    pub jump_table: Option<JumpTable>,
    /// An anonymous read-only data BLOB this function references (`@N` in
    /// `.rodata` via ADDR16_HA/LO — e.g. __strtold's 42-byte zero table).
    /// Numbered like the jump table: `anonymous_offset` past the function's
    /// running `@N` counter, BEFORE its pool constants.
    pub anonymous_rodata: Option<AnonymousRodata>,
    /// Callees that order with the PROTOTYPED externals despite being
    /// unprototyped in OUR model: a `#pragma cplusplus` inline helper called
    /// under its MANGLED name (pikmin s_ldexp's __fpclassifyd__Fd) — its
    /// dropped definition preceded the function, so mwcc's symbol is NOT
    /// implicit-at-call-site. Binding stays GLOBAL UND.
    pub local_undefined_callees: Vec<String>,
    /// Referenced symbol names (globals and callees) in mwcc's symbol-table order —
    /// an AST traversal, NOT `.text` reference order. The writer assigns this
    /// function's external/global symbol indices in this order (see the codegen's
    /// `symbol_order`), falling back to relocation order for anything not listed.
    pub symbol_order: Vec<String>,
    /// Callee names this function references that were IMPLICITLY declared (called with
    /// no prior prototype — K&R style). mwcc creates an implicit callee's symbol at its
    /// first call site, INSIDE the function body, so it is emitted AFTER the function's
    /// own symbol (a prototyped/explicit external is created at its file-scope declaration
    /// and precedes the function). The writer uses this to place such callees after the
    /// function symbol instead of before.
    pub implicit_external_callees: Vec<String>,
    /// A Metrowerks inline-`asm` function: its instructions were assembled
    /// verbatim. mwcc does NOT catalog hand-written asm in the `.mwcats.text`
    /// section (only compiler-generated functions are cataloged), so the writer
    /// excludes it from the mwcats records and relocations.
    pub is_asm: bool,
}

impl MachineFunction {
    pub fn new(name: impl Into<String>) -> Self {
        MachineFunction {
            name: name.into(),
            section: None,
            instructions: Vec::new(),
            relocations: Vec::new(),
            constants: Vec::new(),
            string_literals: Vec::new(),
            new_string_count: 0,
            new_string_names: Vec::new(),
            is_static: false,
            implicit_materialized: false,
            weak_inline: false,
            is_weak: false,
            static_locals: Vec::new(),
            has_conversion: false,
            constant_number_gaps: Vec::new(),
            keep_named_const_scalars: Vec::new(),
            phantom_externals: Vec::new(),
            has_float_branch: false,
            anonymous_label_bump: 0,
            post_constant_label_bump: 0,
            pre_scheduled: false,
            frame: None,
            jump_table: None,
            anonymous_rodata: None,
            local_undefined_callees: Vec::new(),
            symbol_order: Vec::new(),
            implicit_external_callees: Vec::new(),
            is_asm: false,
        }
    }

    /// Intern a pool constant, returning its index. Equal constants share one slot
    /// (mwcc pools identical constants).
    pub fn intern_constant(&mut self, bits: u64, byte_width: u8) -> usize {
        self.intern_constant_slotted(bits, byte_width, false, false)
    }

    /// Intern a constant that numbers at the function's STATIC-LOCAL slot
    /// (an initialized auto array's pooled word image).
    pub fn intern_constant_static_slot(&mut self, bits: u64, byte_width: u8) -> usize {
        self.intern_constant_slotted(bits, byte_width, true, true)
    }

    /// Intern an auto-array image that numbers in the POOL BLOCK but whose
    /// symbol still leads the owning static function (ww's variant).
    pub fn intern_constant_image(&mut self, bits: u64, byte_width: u8) -> usize {
        self.intern_constant_slotted(bits, byte_width, false, true)
    }

    fn intern_constant_slotted(&mut self, bits: u64, byte_width: u8, static_slot: bool, image: bool) -> usize {
        let constant = PoolConstant { bits, byte_width, static_slot, image };
        if let Some(index) = self.constants.iter().position(|existing| *existing == constant) {
            return index;
        }
        self.constants.push(constant);
        self.constants.len() - 1
    }

    /// Encode the whole function to big-endian `.text` bytes. Forward conditional
    /// branches are resolved here from instruction positions.
    pub fn encode_text(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.instructions.len() * 4);
        for (index, instruction) in self.instructions.iter().enumerate() {
            let word = match *instruction {
                Instruction::BranchConditionalForward { options, condition_bit, target } => {
                    let offset = (target as i64 - index as i64) * 4;
                    (16 << 26) | ((options as u32) << 21) | ((condition_bit as u32) << 16) | ((offset as u32) & 0xfffc)
                }
                Instruction::Branch { target } => {
                    let offset = (target as i64 - index as i64) * 4;
                    (18 << 26) | ((offset as u32) & 0x03ff_fffc)
                }
                ref other => other.encode(),
            };
            bytes.extend_from_slice(&word.to_be_bytes());
        }
        bytes
    }
}
