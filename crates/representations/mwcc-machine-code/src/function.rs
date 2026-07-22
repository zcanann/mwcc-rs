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
    /// NEVER dedupe this entry against an equal earlier constant — mwcc can
    /// emit twin pool slots for the same value (strtold's two zero doubles
    /// @296/@297 at distinct `.sdata2` offsets).
    pub force_new: bool,
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

/// A function-scoped object with static storage. Keeping this as a named data
/// object (rather than an expanding tuple) lets the syntax and object stages
/// preserve initialized bytes and their relocations independently of codegen.
#[derive(Debug, Clone)]
pub struct StaticLocal {
    pub name: String,
    pub initial_bytes: Option<Vec<u8>>,
    pub size: u32,
    pub alignment: u32,
    pub is_const: bool,
    /// `(byte offset, target symbol, addend)` for `R_PPC_ADDR32` entries.
    pub relocations: Vec<(u32, String, i32)>,
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
    /// Pack this function's literals into one NUL-separated `@stringBaseN`
    /// object. Later absolute-addressing compilers use one base relocation plus
    /// interior offsets instead of one anonymous object per literal.
    pub packed_string_literals: bool,
    /// The count of NEW (non-reused) strings this function contributes to the unit's
    /// anonymous-`@N` pool — set by the unit's string resolver. The object writer
    /// advances its per-function `@N` counter by this at the FRONT of the function's
    /// block (strings precede the function's constants and unwind entries).
    pub new_string_count: u32,
    /// When set to K, this function's NEW pooled strings take their `@N`
    /// numbers AFTER its first K pool constants instead of at the block front
    /// (bfbb's __dec2num: constants @1539-1541, THEN the string @1542 —
    /// creation order inside the body).
    pub string_number_after_constants: Option<u32>,
    /// Number (and emit) this function's NEW strings after the first K of its
    /// anonymous `.rodata` blobs, with a GAP first: `Some((k, gap))` — strtold's
    /// "NAN(" @53 sits between "INFINITY" @39 and the 32-byte template @54.
    pub string_number_after_rodata: Option<(u32, u32)>,
    /// The `@N` names of the NEW strings this function introduces, in front-of-block
    /// order — set by the unit's string resolver alongside `new_string_count`. The
    /// object writer emits a LOCAL symbol for each at the FRONT of the function's `@N`
    /// block (before its constants and unwind entries), so a second string-bearing
    /// function interleaves its string symbol per-function the way mwcc does.
    pub new_string_names: Vec<String>,
    /// A `static` (file-local) function — emitted with a LOCAL `STT_FUNC` symbol
    /// rather than a global one.
    pub is_static: bool,
    /// This STATIC function's static-local symbols LEAD its FUNC symbol
    /// (mwcc creates them at their in-body declaration points — measured on
    /// mp4 alloc's get_malloc_pool: protopool$129, init$130, then the FUNC).
    /// ac uart measured the opposite (FUNC first), so this is per-capture.
    pub static_locals_lead: bool,
    /// See mwcc_syntax_trees::Function::text_deferred — `.text` lays out
    /// AFTER the next non-deferred function; the symbol stays at source
    /// position.
    pub text_deferred: bool,
    /// A static function MATERIALIZED from an implicitly-declared inline: its
    /// call relocations bind the UND ghost and its local symbol trails its
    /// static locals (ww uart).
    pub implicit_materialized: bool,
    /// A WEAK function materialized from a PLAIN inline: its .comment flag is
    /// the weak-OBJECT 0x0d, not declspec-weak's 0x0e (strikers mbstowcs).
    pub weak_inline: bool,
    pub is_weak: bool,
    /// The function's STATIC LOCALS, emitted as `name$K` LOCAL objects with K
    /// taken from the function's @N sequence front.
    pub static_locals: Vec<StaticLocal>,
    /// Extra `$N` adjustment for THIS function's capture-pushed static locals
    /// (unmeasured inline label consumption before the owner — _alloc's
    /// protopool$109/init$110 sit +50 past the natural counter).
    pub static_local_adjust: i64,
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
    /// Unit-analysis ordinals consumed only by fragmented debug metadata.
    /// These precede the first function's debug fragment but must not shift
    /// that function's constant/unwind objects.
    pub fragmented_debug_anonymous_bump: u32,
    /// Ordinal work performed in source order before deferred code emission is
    /// reversed. When a later compiled body becomes the physical head, this
    /// amount is transferred to that head without changing this body's pool.
    pub deferred_source_prefix_bump: u32,
    /// Extra `@N` numbers consumed AFTER this function's pooled constants
    /// and before its extab pair (the nested punned-guard's inner block).
    pub post_constant_label_bump: u32,
    /// Override the build-wide anonymous-counter gap after this function.
    /// Most functions use the ABI generation's default; semantically inlined
    /// helper families can retain additional compiler bookkeeping slots.
    pub post_function_anonymous_bump: Option<u8>,
    /// Emitted by the DAG emitter: the order IS the schedule — the legacy
    /// post-allocation scheduling passes must not touch it.
    pub pre_scheduled: bool,
    /// Frame metadata for the unwind tables; `None` for a leaf with no frame.
    pub frame: Option<FrameInfo>,
    /// A dense `switch`'s jump table; `None` unless the function dispatches through
    /// one. The writer materializes it as an anonymous `@N` object in `.data`.
    pub jump_tables: Vec<JumpTable>,
    /// An anonymous read-only data BLOB this function references (`@N` in
    /// `.rodata` via ADDR16_HA/LO — e.g. __strtold's 42-byte zero table).
    /// Numbered like the jump table: `anonymous_offset` past the function's
    /// running `@N` counter, BEFORE its pool constants.
    pub anonymous_rodata: Vec<AnonymousRodata>,
    /// This function's NEW string literals are CONST (`.sdata2`) — a const
    /// char array read as data (strtold's "NAN(").
    pub strings_are_const: bool,
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
    /// Referenced names known to designate functions (direct callees or
    /// address-taken function symbols). The object writer uses this type channel
    /// to distinguish an absolute function address from absolute data.
    pub referenced_function_symbols: Vec<String>,
    /// Capture-only pin for the translation unit's interleaved LOCAL data/function
    /// symbol creation order. Most units use the writer's general policy; rare
    /// legacy TUs create zero statics and static functions in a first-reference /
    /// definition timeline that an exact capture records explicitly.
    pub local_symbol_order: Vec<String>,
    /// Callee names this function references that were IMPLICITLY declared (called with
    /// no prior prototype — K&R style). mwcc creates an implicit callee's symbol at its
    /// first call site, INSIDE the function body, so it is emitted AFTER the function's
    /// own symbol (a prototyped/explicit external is created at its file-scope declaration
    /// and precedes the function). The writer uses this to place such callees after the
    /// function symbol instead of before.
    pub implicit_external_callees: Vec<String>,
    /// Implicit callees that build 163 creates before a referenced data target
    /// while lowering a call-result conversion. They remain after the current
    /// function symbol, but precede that function's explicit data references.
    pub early_implicit_external_callees: Vec<String>,
    /// A Metrowerks inline-`asm` function: its instructions were assembled
    /// verbatim. mwcc does NOT catalog hand-written asm in the `.mwcats.text`
    /// section (only compiler-generated functions are cataloged), so the writer
    /// excludes it from the mwcats records and relocations.
    pub is_asm: bool,
    /// Inline-`asm` `entry <name>` points: additional GLOBAL symbols at `.text`
    /// offsets within this function (the runtime's `_savefpr_14` … register save/
    /// restore entry points). Each pairs the symbol name with its instruction index.
    pub entry_points: Vec<(String, usize)>,
    /// Defined under `#pragma force_active on`: the function symbol and its entry
    /// symbols carry a `.comment` attribute (0x00080000). `false` for the common case.
    pub force_active: bool,
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
            packed_string_literals: false,
            new_string_count: 0,
            string_number_after_constants: None,
            string_number_after_rodata: None,
            new_string_names: Vec::new(),
            is_static: false,
            static_locals_lead: false,
            text_deferred: false,
            implicit_materialized: false,
            weak_inline: false,
            is_weak: false,
            static_locals: Vec::new(),
            static_local_adjust: 0,
            has_conversion: false,
            constant_number_gaps: Vec::new(),
            keep_named_const_scalars: Vec::new(),
            phantom_externals: Vec::new(),
            has_float_branch: false,
            anonymous_label_bump: 0,
            fragmented_debug_anonymous_bump: 0,
            deferred_source_prefix_bump: 0,
            post_constant_label_bump: 0,
            post_function_anonymous_bump: None,
            pre_scheduled: false,
            frame: None,
            jump_tables: Vec::new(),
            anonymous_rodata: Vec::new(),
            strings_are_const: false,
            local_undefined_callees: Vec::new(),
            symbol_order: Vec::new(),
            referenced_function_symbols: Vec::new(),
            local_symbol_order: Vec::new(),
            implicit_external_callees: Vec::new(),
            early_implicit_external_callees: Vec::new(),
            is_asm: false,
            entry_points: Vec::new(),
            force_active: false,
        }
    }

    /// Anonymous-label work charged before this function's pool/unwind block.
    /// Object and fragmented-debug numbering must consume the same value.
    pub fn object_anonymous_bump(&self) -> u32 {
        u32::from(self.has_conversion)
            + 3 * u32::from(self.has_float_branch)
            + self.anonymous_label_bump
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

    /// Intern a FRESH pool slot even when an equal constant already exists
    /// (mwcc's occasional twin slots for one value — strtold's zero doubles).
    pub fn intern_constant_new(&mut self, bits: u64, byte_width: u8) -> usize {
        self.constants.push(PoolConstant {
            bits,
            byte_width,
            static_slot: false,
            image: false,
            force_new: true,
        });
        self.constants.len() - 1
    }

    fn intern_constant_slotted(
        &mut self,
        bits: u64,
        byte_width: u8,
        static_slot: bool,
        image: bool,
    ) -> usize {
        let constant = PoolConstant {
            bits,
            byte_width,
            static_slot,
            image,
            force_new: false,
        };
        if let Some(index) = self
            .constants
            .iter()
            .position(|existing| *existing == constant)
        {
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
                Instruction::BranchConditionalForward {
                    options,
                    condition_bit,
                    target,
                } => {
                    let offset = (target as i64 - index as i64) * 4;
                    (16 << 26)
                        | ((options as u32) << 21)
                        | ((condition_bit as u32) << 16)
                        | ((offset as u32) & 0xfffc)
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
