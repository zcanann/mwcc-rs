//! The `Generator` — codegen state — plus its small accessors. The emit
//! logic lives in the sibling theme modules, each a further `impl Generator`.

use crate::analysis::*;
use crate::condition_float_cache::ConditionFloatCache;
use crate::condition_global_cache::ConditionGlobalValue;
use crate::condition_member_cache::ConditionMemberCache;
use crate::control_flow::WidePairMaskCache;
use crate::{InlineBodySet, InlineSummaries};
use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::{
    Instruction, MachineFunction, Relocation, RelocationKind, RelocationTarget,
};
use mwcc_syntax_trees::{BinaryOperator, Expression, Pointee, Type, UnaryOperator};
use mwcc_versions::{Behavior, GlobalAddressing};
use mwcc_vreg::{Class, Reg, RegisterConstraints, VirtualRegister};
use std::collections::{HashMap, HashSet};

/// The scratch register mwcc spills the secondary operand of a binary node into.
pub(crate) const GENERAL_SCRATCH: u8 = 0; // r0
pub(crate) const FLOAT_SCRATCH: u8 = 0; // f0

/// Per-register-file virtual identity cursors.
///
/// Instruction fields encode a virtual ID together with an operand's
/// machine-described class. General and floating registers therefore have
/// independent 224-ID namespaces; sharing one cursor needlessly halved that
/// capacity and made long mixed GPR/FPR functions hit the transitional field
/// ceiling.
#[derive(Clone, Copy, Default)]
pub(crate) struct VirtualCursors {
    pub(crate) general: u32,
    pub(crate) float: u32,
}

impl VirtualCursors {
    fn next(&mut self, class: Class) -> VirtualRegister {
        let cursor = match class {
            Class::General => &mut self.general,
            Class::Float => &mut self.float,
        };
        let register = VirtualRegister::new(*cursor, class);
        *cursor += 1;
        register
    }

    fn contains(self, register: VirtualRegister) -> bool {
        register.id
            < match register.class {
                Class::General => self.general,
                Class::Float => self.float,
            }
    }
}

/// Canonical value of a pool literal used by a floating comparison. Keeping
/// the comparison precision in the key prevents a preloaded `0.0f` from being
/// consumed by a later double comparison with the same source spelling.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum FloatCompareLiteralKey {
    Single(u32),
    Double(u64),
}

pub(crate) fn float_compare_literal_key(
    operand: &Expression,
    double: bool,
) -> Option<FloatCompareLiteralKey> {
    let value = match operand {
        Expression::FloatLiteral(value) => *value,
        Expression::IntegerLiteral(value) => *value as f64,
        _ => return None,
    };
    Some(if double {
        FloatCompareLiteralKey::Double(value.to_bits())
    } else {
        FloatCompareLiteralKey::Single((value as f32).to_bits())
    })
}

#[derive(Clone, Copy)]
pub(crate) struct PreloadedFloatCompareLiteral {
    pub(crate) key: FloatCompareLiteralKey,
    pub(crate) register: u8,
    pub(crate) remaining_uses: usize,
    pub(crate) reuse_for_following_value: bool,
}

pub(crate) struct StructuredFloatHandoff {
    pub(crate) name: String,
    pub(crate) source: u8,
    pub(crate) destination: u8,
    pub(crate) emitted: bool,
}

/// Original memory value retained beside a conditionally transformed local.
/// Build 163 can compare the source again without reloading it while the local
/// occupies a separate FPR for a later call argument.
#[derive(Clone)]
pub(crate) struct RetainedFloatCompareValue {
    pub(crate) expression: Expression,
    pub(crate) register: u8,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum ValueClass {
    General,
    Float,
}

pub(crate) struct Location {
    pub(crate) class: ValueClass,
    pub(crate) register: u8,
    pub(crate) signed: bool,
    /// Integer width in bits (8/16/32); narrow values are extended when read.
    pub(crate) width: u8,
    /// For a pointer value, what it points to (so `*p` picks the right load).
    pub(crate) pointee: Option<Pointee>,
    /// For a struct pointer, the struct's byte size — the stride for scaled pointer
    /// arithmetic (`p + n`, `p++`). `None` for scalar pointers (which scale by the
    /// `pointee` size) and non-pointers.
    pub(crate) stride: Option<u32>,
}

/// The float-composition channel (see `Generator::float`).
#[derive(Default)]
pub(crate) struct FloatContext {
    /// The float DAG tail reloads x from this frame offset (the fctiwz
    /// punned-guard composition): x's references become a frame lfd node
    /// (value id 9) and f1 frees for the chain.
    pub(crate) reload_x: Option<i16>,
    /// Extra float bindings for a DAG tail: shared dual-tail locals already
    /// materialized in registers (name -> FPR).
    pub(crate) pseudo_params: Vec<(String, u8)>,
    /// A double local defined by a CONDITIONAL diamond ahead of the float
    /// tail: the tail's DAG allocates it as a window-top tier value (a
    /// PHANTOM node, value id 8, emitting nothing) and reports the assigned
    /// register back so the diamond arms load into it.
    pub(crate) phantom_local: Option<String>,
    pub(crate) phantom_register: Option<u8>,
    /// A double local resident in a FRAME slot (value id 7).
    pub(crate) frame_local: Option<(String, i16)>,
    /// The BIG-constant dual compare: (lis high, addi low, ix register).
    pub(crate) dual_compare: Option<(i16, i16, u8)>,
    /// The k_cos ELSE composition payload.
    pub(crate) else_composition: Option<FloatElseComposition>,
}

/// The k_cos else-branch composition payload (set by the punned arm,
/// consumed by the dual arm's else phase).
#[derive(Clone)]
pub(crate) struct FloatElseComposition {
    /// The inner compare's lis half (`lis r0, high; cmpw ix, r0`).
    pub(crate) compare_high: i16,
    /// The skip branch to the diamond's else arm (ble for Greater).
    pub(crate) skip_options: u8,
    pub(crate) skip_bit: u8,
    /// The preserved ix register (the compare's A side, the addis source).
    pub(crate) ix_register: u8,
    /// The freed raw-word register the addis result lands in (r3 modern,
    /// r0 for the legacy frame convention).
    pub(crate) addis_target: u8,
    /// Whether the high word is stored before r0 is reused to materialize
    /// the zero low word.
    pub(crate) store_high_before_zero: bool,
    /// The diamond's then-arm literal (qx = 0.28125).
    pub(crate) then_bits: u64,
    /// The addis immediate (ix - C, C a lis-able constant; shift = -C>>16).
    pub(crate) addis_shift: i16,
    /// The diamond local's name + frame offset.
    pub(crate) qx_name: String,
    pub(crate) qx_offset: i16,
    /// The else-only fold-away locals (hz, a) with their initializers.
    pub(crate) else_locals: Vec<mwcc_syntax_trees::LocalDeclaration>,
}

/// A variable whose address is taken: it lives in a stack-frame slot rather than
/// a register. `&v` is `addi d, r1, offset`, and a type-punned access `*(t*)&v`
/// is a displacement load/store from `r1`.
#[derive(Clone, Copy)]
pub(crate) struct FrameSlot {
    /// Byte offset from the stack pointer (`r1`).
    pub(crate) offset: i16,
    /// Whether the variable is a float/double (spilled with `stfd`/`stfs`).
    pub(crate) class: ValueClass,
    /// Byte size of the variable (4 or 8).
    pub(crate) size: u32,
    /// Source-level type of the stored value. This is deliberately separate
    /// from `size`: narrow scalar parameters and locals occupy ABI-sized
    /// lanes, but must still use byte/halfword loads and stores.
    pub(crate) value_type: Type,
    /// The incoming argument register, if this is a spilled parameter.
    pub(crate) parameter_register: Option<u8>,
    /// Whether this slot is a local array (`int buf[N];`): in value position the
    /// name decays to the slot's *address* (`addi d,r1,offset`) rather than a load.
    pub(crate) is_array: bool,
}

/// Build 163 callee-saved frame bookkeeping that cannot be recovered from the
/// allocated home alone. Most layouts follow the home's value origin. A
/// producing call that directly consumes entry parameters reserves one extra
/// lane even though its saved home is first defined by the call result.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum LegacyCalleeSavedFrameLayout {
    #[default]
    InferFromValueOrigin,
    RetainEntryParameterTable,
    /// A source guard that records one saved entry parameter before a call and
    /// retains another parameter across that call. Build 163 keeps both the
    /// condition-materialization lane and its ordinary entry-value table.
    RetainGuardedEntryParameterTable,
    RetainEagerLocalLane,
    /// A deferred local first materialized inside a guarded arm, or a deferred
    /// pointer first materialized after a call, remains live across later calls.
    /// Build 163 reserves one optimizer lane even when the value stays in a
    /// saved register.
    RetainDeferredLocalLane,
    /// Retain the ordinary incoming-value table and one additional optimizer
    /// lane for a source local whose assignment was eliminated as unobserved.
    RetainEntryParameterTableAndDeferredLocalLane,
    /// A global-member address first materialized inside a guarded arm remains
    /// live across a later call. Build 163 keeps the same optimizer lane as a
    /// deferred local, without enabling deferred-local issue-order schedules.
    RetainDeferredGlobalMemberAddressLane,
    ReserveForwardedParameterLane,
    /// Source-owned stack storage already represents all retained optimizer
    /// values, including eliminated inline bindings. Do not append inferred
    /// entry or inline lanes during linkage-first normalization.
    PreserveLogicalSize,
}

/// One frame-resident aggregate value copied into an EABI outgoing-argument
/// slot before a direct call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StructuredAggregateArgumentCopy {
    pub(crate) argument_index: usize,
    pub(crate) local: String,
    pub(crate) copy_offset: i16,
}

/// Complete ownership of one structured body's outgoing aggregate-copy area.
///
/// Keeping the callee and every copy in one plan prevents an unrelated call
/// from accidentally consuming frame slots reserved for the terminal call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StructuredAggregateCallCopyPlan {
    pub(crate) callee: String,
    pub(crate) copies: Vec<StructuredAggregateArgumentCopy>,
    pub(crate) total_bytes: i16,
}

#[derive(Debug, Clone)]
pub(crate) struct StructuredGlobalIndexCache {
    pub(crate) global: String,
    pub(crate) index: String,
    pub(crate) stride: u32,
    pub(crate) scaled: u8,
    pub(crate) retained_element: Option<u8>,
    pub(crate) retained_element_initialized: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct StructuredGlobalBaseCache {
    pub(crate) global: String,
    pub(crate) register: u8,
}

#[derive(Debug, Clone)]
pub(crate) struct StructuredGlobalMemberAddressCache {
    pub(crate) global: String,
    pub(crate) total_size: u32,
    pub(crate) offset: i16,
    pub(crate) register: u8,
    pub(crate) initialized: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct DataSectionAnchorPlan {
    /// Full `.data` or `.bss` objects addressed through one translation-unit
    /// section anchor.
    /// Their exact section offsets are assigned after every function-created
    /// string and table is known, so each D-form use carries a late fixup.
    pub(crate) symbols: HashSet<String>,
    pub(crate) anchor_symbol: String,
    pub(crate) register: Option<u8>,
}

#[derive(Debug, Clone)]
pub(crate) struct TransientGlobalIndexBase {
    pub(crate) global: String,
    pub(crate) index: String,
    pub(crate) stride: u32,
    pub(crate) register: u8,
}

pub(crate) struct Generator {
    /// This function is a VARIADIC definition — only a capture may emit it
    /// (the register-save prologue is unmodeled in general codegen).
    pub(crate) variadic_definition: bool,
    /// Direct callees declared variadic. EABI callers set CR bit 6 when floating
    /// argument registers are live and clear it for general-only calls.
    pub(crate) variadic_callees: HashSet<String>,
    pub(crate) output: MachineFunction,
    /// Branch labels awaiting resolution — the multi-block emission substrate.
    /// Resolved into `output.instructions` once body emission completes.
    pub(crate) labels: mwcc_vreg::Labels,
    pub(crate) locations: HashMap<String, Location>,
    /// File-scope globals by name; a reference to one loads from the small-data
    /// area (an `R_PPC_EMB_SDA21` relocation off r13, the `0(r0)` placeholder).
    pub(crate) globals: HashMap<String, Type>,
    /// File-scope objects whose address may be formed, including const scalar
    /// declarations deliberately withheld from [`Self::globals`] because their
    /// value reads require folding. Addressability is independent of whether a
    /// direct value load is currently supported: `&extern_const_object` still
    /// names real storage.
    pub(crate) addressable_globals: HashMap<String, Type>,
    /// Volatile globals are kept separate from the compact type map so semantic
    /// rewrites can prove that eliminating a reload is legal.
    pub(crate) volatile_globals: HashSet<String>,
    /// Total byte size of each file-scope *array* global, by name. Drives the
    /// per-symbol address mode when subscripting it: a small array (≤ 8 bytes,
    /// `.sdata`) materializes via SDA21, a large one (`.data`/`.bss`) via ADDR16.
    pub(crate) global_array_sizes: HashMap<String, u32>,
    /// Every file-scope array, including `extern T table[]` declarations whose
    /// extent is unknown in this translation unit. Keep identity separate from
    /// `global_array_sizes`: optimizations may require a proven finite bound,
    /// while type classification and address materialization only need to know
    /// that the symbol is an array.
    pub(crate) global_arrays: HashSet<String>,
    /// A source-stable global struct-array subscript whose scaled index is
    /// retained across calls by the structured body owner.
    pub(crate) structured_global_index_cache: Option<StructuredGlobalIndexCache>,
    /// A call-free leading member cluster shares one global aggregate address.
    /// The virtual register's last member load ends the live range before calls.
    pub(crate) structured_global_base_cache: Option<StructuredGlobalBaseCache>,
    /// Exact scalar-global member address retained across a possible call.
    pub(crate) structured_global_member_address_cache:
        Option<StructuredGlobalMemberAddressCache>,
    pub(crate) data_section_anchor: Option<DataSectionAnchorPlan>,
    /// The `.data` anchor occupies a deferred value's home before that value is
    /// defined. Linkage-first frame normalization shifts the retained entry
    /// lane into existing alignment slack instead of growing the frame.
    pub(crate) data_section_anchor_reuses_deferred_home: bool,
    /// The structured frame emitted a pooled automatic-array copy transaction.
    /// Later physical scheduling uses this provenance instead of inferring the
    /// owner from common instructions such as `stmw`.
    pub(crate) structured_array_pool_emitted: bool,
    /// Structured lowering recognized the pairwise object-collision loop whose
    /// entry is finalized after allocated FPR frame materialization.
    pub(crate) structured_object_collision_loop_entry: bool,
    /// Queueing callee from a semantically recognized inlined callback-wait
    /// transaction. The final physical pass uses this provenance instead of
    /// trying to rediscover the expanded body from the original source AST.
    pub(crate) structured_sequenced_callback_wait_starter: Option<String>,
    /// Conditional edges owned by a retained source switch dispatcher. They
    /// can have the same local shape as a structured `if (...) goto`, but MWCC
    /// preserves the switch's two-edge leaf rather than folding it.
    pub(crate) structured_switch_dispatch_conditionals: HashSet<usize>,
    /// A semantic transaction whose final CFG cleanup removes unreachable
    /// forwarding blocks and no-op fallthrough branches. Ordinary functions
    /// retain that optimizer residue.
    pub(crate) structured_cfg_cleanup_owner: bool,
    /// A structured sequence of direct sends followed by empty call-poll loops.
    /// MWCC removes their pre-test fallthrough branches without applying the
    /// generic eight-byte alignment used by standalone polling loops.
    pub(crate) structured_repeated_call_poll_owner: bool,
    /// A nonvolatile global pointer loaded immediately before a guarded switch
    /// and consumed at the start of several mutually exclusive arms.
    pub(crate) structured_shared_switch_global_value: Option<(String, u8)>,
    /// Complete global element base shared only within the current call's
    /// argument transaction. It is reset before every argument list.
    pub(crate) transient_global_index_base: Option<TransientGlobalIndexBase>,
    /// Defined, uninitialized file-scope objects routed to full `.bss`.
    ///
    /// Aggregate-wide schedules may address these through `...bss.0`; keeping
    /// storage provenance separate from the executable type map prevents an
    /// extern or initialized aggregate from being claimed by that schedule.
    pub(crate) full_bss_globals: HashSet<String>,
    /// Registers holding live values that must not be clobbered while a sibling
    /// sub-expression is being evaluated. The allocator draws temporaries from
    /// the registers outside this set.
    pub(crate) reserved: HashSet<u8>,
    /// Stack frame size in bytes (0 = leaf function, no frame). Set when an
    /// operation needs scratch stack space (e.g. an int/float conversion).
    pub(crate) frame_size: i16,
    /// The float-composition CHANNEL: everything an arm sets around a claim
    /// (and must restore afterward). One struct so a single
    /// `std::mem::take`/restore covers the whole set — missed per-field
    /// restores caused three real bugs across the float campaign.
    pub(crate) float: FloatContext,
    /// File-scope `static const double T[] = {...}` coefficient tables:
    /// constant-index reads become lfd's off ONE lis/addi ADDR16 base.
    pub(crate) double_tables: std::collections::HashSet<String>,
    /// The resolved codegen decisions for the configuration we are reproducing.
    /// Every version- or flag-varying choice is read from this one flat set,
    /// computed once from the build's profile and flags — never re-derived in
    /// instruction selection.
    pub(crate) behavior: Behavior,
    /// Source spelling of this function's return type when compact executable
    /// types have merged it with a storage-equivalent scalar.
    pub(crate) return_source_fundamental: Option<mwcc_syntax_trees::SourceFundamentalType>,
    /// Source spelling of each callable's return type. This keeps forwarding a
    /// `bool` result distinct from converting an `unsigned char` result to bool.
    pub(crate) call_return_fundamentals:
        HashMap<String, mwcc_syntax_trees::SourceFundamentalType>,
    /// The target's register-allocation rules — the allocatable pools and scratch.
    /// The free-register helpers draw from here, so the pools have one authoritative
    /// home (shared with the future allocator) rather than literals in placement.
    pub(crate) constraints: RegisterConstraints,
    /// Whether the function makes a call: it then saves/restores the link register
    /// around a stack frame (the non-leaf prologue/epilogue).
    pub(crate) non_leaf: bool,
    /// Whether an inline-assembly definition appeared before this function.
    ///
    /// Build 163 carries scheduler state across that source-order boundary:
    /// runtime helpers following startup asm retain logical argument copies and
    /// keep their condition after the complete linkage-first prologue.
    pub(crate) preceded_by_asm: bool,
    /// Callee-saved FLOAT registers the arm saves (f31 descending) — the
    /// extab's saved-FPR count.
    pub(crate) callee_saved_float: u8,
    /// The next virtual-register id in each independent register file.
    pub(crate) virtual_cursors: VirtualCursors,
    /// Per-virtual placement hints: registers the allocator must avoid for a
    /// given virtual id. Selection records these (e.g. "a comparison operand must
    /// avoid the destination") so the allocation pass reproduces mwcc's coalescing
    /// of result-path temporaries onto the destination register.
    pub(crate) register_avoid: HashMap<VirtualRegister, Vec<u8>>,
    /// Consumer-tree PREFERENCES: virtual id -> the register its consumer wants
    /// (Phase D policy #1); honored by LinearScan when free, pool order otherwise.
    pub(crate) register_prefer: HashMap<VirtualRegister, u8>,
    /// Return type of each callable name (prototypes + definitions), so a call's
    /// result type is known — e.g. `(float)cos(x)` rounds a double with `frsp`.
    pub(crate) call_return_types: HashMap<String, Type>,
    /// Fixed-address ARRAY globals (`vu32 __EXIRegs[16] : 0xCC006800;`): name -> (address, element
    /// type). A `name[i]` subscript lays out mwcc's array form (`lis; addi; lwzx`) off the constant base.
    pub(crate) fixed_address_arrays: HashMap<String, (u32, Type)>,
    /// Scalar and aggregate MWCC absolute-address declarations. The expression parser lowers
    /// their uses to constant-address dereferences, so this side table preserves the declaration
    /// origin for schedules that differ from an explicit pointer cast.
    pub(crate) fixed_address_objects: HashMap<String, u32>,
    /// Row byte-strides of flattened multi-dimensional FRAME arrays (`float m[3][4]`
    /// -> 16): `m[k]` in value position is the ROW ADDRESS `slot + k*stride`.
    pub(crate) frame_row_bytes: HashMap<String, u16>,
    /// Scalar element types for those flattened rows. This is available before
    /// structured frame slots are installed, so liveness/type analysis and
    /// final emission classify nested subscripts identically.
    pub(crate) frame_row_pointees: HashMap<String, Pointee>,
    /// PASS-ARC STEP 2: when a whole-body fill emitted its values as virtuals, the
    /// DESCENDING allocation window's top register (r(N+2) for an N-store fill).
    /// `None` keeps the default LinearScan policy.
    pub(crate) descending_allocation_top: Option<u8>,
    /// Skipped inline definitions' names — a body calling one defers after
    /// the exact-match templates decline (mwcc inlines; a bl would be wrong).
    pub(crate) skipped_inline_names: std::collections::HashSet<String>,
    /// File-scope PROTOTYPE-only names (external declarations, not definitions in
    /// this TU). A call to one is a genuine external `bl`; a name absent here that
    /// the TU defines (e.g. a `static`) may be inlined by mwcc.
    pub(crate) prototyped_names: std::collections::HashSet<String>,
    /// PLAIN-inline functions our parser MATERIALIZED as weak globals. mwcc may
    /// instead re-inline a trivial one at its call sites (ww's mbstowcs folds
    /// callers to `blr`), so a NATIVE caller defers — only a capture claim
    /// knows the real bytes.
    pub(crate) weak_materialized_names: std::collections::HashSet<String>,
    /// Parameter types of each callable name, so a call places each argument in the
    /// register its parameter requires (a float parameter takes f1.., an integer
    /// takes r3..) and a type mismatch is detected rather than silently mis-passed.
    pub(crate) call_parameter_types: HashMap<String, Vec<Type>>,
    /// Retained skipped-inline bodies eligible for conservative statement-level
    /// composition after whole-function captures have declined.
    pub(crate) inline_bodies: InlineBodySet,
    /// String bytes introduced by a reachable retained-inline body, mapped to
    /// MWCC's weak `@STRING@<inline owner>` object identity.
    pub(crate) inline_string_symbols: HashMap<Vec<u8>, String>,
    /// Semantically verified summaries of other definitions in this translation
    /// unit. Exact inline compositions consult these instead of callee names.
    pub(crate) inline_summaries: InlineSummaries,
    /// A global just stored, with the register holding the stored value and the
    /// instruction count at the moment of the store. A subsequent read of the
    /// global may reuse that register instead of reloading — but only while no
    /// instruction has been emitted since (so the value is provably still there).
    /// The resolved version policy decides whether the compiler generation takes
    /// advantage of that live value.
    pub(crate) stored_globals: HashMap<String, (u8, usize)>,
    /// Nonvolatile pointer globals retained while one side-effect-free branch
    /// condition reads several of their members. The scope owner restores this
    /// map before emitting the guarded body, so reuse cannot cross a call.
    pub(crate) condition_global_values: HashMap<String, ConditionGlobalValue>,
    /// Float memory loads retained only along a side-effect-free condition's
    /// fallthrough edge into the next guard.
    pub(crate) condition_float_cache: ConditionFloatCache,
    /// Integer member loads retained only across the fallthrough terms of one
    /// side-effect-free logical-AND condition.
    pub(crate) condition_member_cache: ConditionMemberCache,
    /// Known-zero high mask value carried only down a nested mask test's false
    /// edge. True arms may contain calls, so their structured owner never sees
    /// this cache.
    pub(crate) wide_pair_mask_cache: WidePairMaskCache,
    /// Retained constant-address base, keyed by the materialized high half.
    ///
    /// A fresh virtual lets liveness extend the base across later accesses with
    /// the same high half inside one call-free region. Calls invalidate the map:
    /// retaining a base across them is a frame-cost decision that belongs in a
    /// future whole-function planner, while the ordinary MWCC schedule
    /// rematerializes it. A control-flow boundary after the last access likewise
    /// ends the region, allowing a later high half to reuse the same physical
    /// home. Overlapping high halves still need MWCC's multi-base look-ahead
    /// schedule and therefore defer. Zero-high accesses use r0-as-zero directly
    /// and are not recorded.
    pub(crate) const_address_bases: HashMap<i16, u8>,
    /// Set once a VARIABLE-index subscript store with an already-materialized
    /// value (`a[i] = v`, i not constant) has scaled its index through r0.
    /// mwcc pre-scales the indices of an uninterrupted RUN of those stores, so a
    /// second leaf-value store needs look-ahead. An indexed RHS is a scheduling
    /// barrier of its own (`slwi; lwzx; slwi; stwx`) and starts a fresh run.
    /// Constant-index stores use a displacement and never consult this state.
    pub(crate) emitted_leaf_variable_index_store_since_scratch_barrier: bool,
    /// Minimum cast/mask/shift depth owned by the packed rotate-mask selector.
    /// Ordinary source expressions use three to preserve the shallow legacy
    /// schedules; compiler-created packet invariants temporarily lower it to
    /// two while their complete expression tree is being emitted.
    pub(crate) packed_shift_mask_min_operations: usize,
    /// Float/double constants pre-loaded into fixed FPRs for a distinct-float-constant store
    /// run (`gf=1.0f; gg=2.0f`, or the `double` `lfd` variant): mwcc pre-loads each into a
    /// distinct FPR (`lfs f1,@a; lfs f0,@b; stfs f1,gf; stfs f0,gg`), so `place_store_value`
    /// reuses the pre-loaded FPR by the literal's f64 bits instead of re-pooling/re-loading.
    /// `(FloatLiteral f64 bits, FPR)`; empty outside a run (runs are homogeneous float/double).
    pub(crate) prematerialized_float_constants: Vec<(u64, u8)>,
    /// Pool literals deliberately issued before their comparisons. A
    /// reservation records how many source comparisons reuse the live value.
    pub(crate) preloaded_float_compare_literals: Vec<PreloadedFloatCompareLiteral>,
    pub(crate) released_float_compare_literal_register: Option<u8>,
    /// Build-163 alias split for a local whose first compare uses its f2
    /// initializer while later mutation/call uses consume a preserved f1 copy.
    pub(crate) structured_float_handoff: Option<StructuredFloatHandoff>,
    pub(crate) retained_float_compare_value: Option<RetainedFloatCompareValue>,
    /// Uninitialized float locals whose first definition is a direct call in a
    /// condition. Assignment lowering binds the name to the comparison value
    /// instead of manufacturing a second live range for the same call result.
    pub(crate) transient_condition_float_call_results: HashSet<String>,
    /// Address-taken variables and their stack-frame slots. A name here is
    /// frame-resident: `&v` and type-punned accesses read/write its slot.
    pub(crate) frame_slots: HashMap<String, FrameSlot>,
    /// Outgoing by-value aggregate copies owned by the allocator-backed
    /// structured body. Source object slots remain in `frame_slots`; this plan
    /// describes the separate caller-owned copies below them.
    pub(crate) structured_aggregate_call_copy_plan:
        Option<StructuredAggregateCallCopyPlan>,
    /// Slot offsets STORED THROUGH during emission (a pun store, a writeback).
    /// A spilled float parameter reloads at its return only when its slot is
    /// here — otherwise the value is still live in the incoming register
    /// (measured: `x *= c` reloads, an untouched x does not).
    pub(crate) written_slots: HashSet<i16>,
    /// A register local initialized from a frame-resident pun was substituted
    /// before the frame owner ran. Preserve its legacy frame-layout effect.
    pub(crate) frame_feeding_local_pressure: Option<(usize, usize)>,
    /// Bytes occupied by numeric-conversion scratch images inside an
    /// allocator-owned callee-saved body. Selection places the images at the
    /// old frame end; the ABI normalizer grows the frame enough to keep them
    /// disjoint from the relocated callee-saved homes.
    pub(crate) callee_saved_conversion_bytes: i16,
    /// Next eight-byte stack image assigned to a float-to-integer conversion.
    /// MWCC gives every conversion expression its own image even when the
    /// lifetimes do not overlap. Structured frame owners pre-plan the complete
    /// range; leaf conversion functions grow their frame as images are claimed.
    pub(crate) float_to_int_scratch_next: i16,
    /// Exclusive end of a pre-planned float-to-integer scratch range. Zero
    /// denotes a leaf function whose conversion frame may grow on demand.
    pub(crate) float_to_int_scratch_end: i16,
    /// Next eight-byte image assigned to an integer-to-floating conversion.
    pub(crate) int_to_float_scratch_next: i16,
    /// Exclusive end of a structured body's pre-planned int-to-float range.
    pub(crate) int_to_float_scratch_end: i16,
    /// A structured named local owns each guarded pointer load. Preserve the
    /// local's scratch value and forward it to r3 instead of treating the pair
    /// as two direct member expressions eligible for load elimination.
    pub(crate) preserve_guarded_named_local_values: bool,
    /// When set, a constant store value reuses the scratch register if it already
    /// holds that constant (`scratch_constant`). Enabled only by a planned
    /// scratch-safe constant-store run, which guarantees nothing clobbers the
    /// scratch between stores, so the reuse is provably valid.
    pub(crate) reuse_scratch_constant: bool,
    /// The constant currently materialized in the scratch register, during a
    /// scratch-safe constant-store run.
    pub(crate) scratch_constant: Option<i32>,
    /// Constants pre-materialized into specific registers ahead of a run of
    /// distinct-constant stores, so each store reuses its register rather than
    /// re-materializing (mwcc materializes both values up front, then stores).
    pub(crate) prematerialized_constants: Vec<(i32, u8)>,
    /// Callee-saved general registers this function uses (r31 first, descending) to
    /// hold values live across a call. They are saved high-to-low in the prologue
    /// and reloaded in the epilogue, and drive the unwind table's saved-GPR count.
    pub(crate) callee_saved: Vec<u8>,
    /// Incoming EABI argument footprint in 32-bit words. Build 163 retains this
    /// frontend bookkeeping when it sizes a frame containing entry-materialized
    /// callee-saved values: every pair of argument words occupies one 8-byte lane,
    /// including words belonging to otherwise-unused parameters.
    pub(crate) entry_parameter_words: usize,
    pub(crate) legacy_callee_saved_frame_layout: LegacyCalleeSavedFrameLayout,
    /// Dead call-initializer locals removed from the semantic body. Build 163
    /// still counts their discarded values while sizing callee-saved frame lanes.
    pub(crate) legacy_discarded_call_locals: usize,
    /// Allocator bookkeeping retained after value-returning inline calls have
    /// been expanded away. The linkage-first frame policy owns its placement.
    pub(crate) legacy_inline_expansion_frame_bytes: usize,
    /// Statement-body substitutions composed into this function. General
    /// inline residue is charged at expansion time; structured frames retain
    /// an additional binding block for each substitution.
    pub(crate) inline_statement_body_substitutions: usize,
    /// Late whole-file substitutions that affect final structured schedules
    /// without participating in the ordinary anonymous-symbol stream.
    pub(crate) late_inline_statement_body_substitutions: usize,
    /// Source-level values that MWCC's pre-composition allocator keeps live
    /// across a guarded inline-call diamond. Semantic composition may prove
    /// the call edge cannot reach the later read, but allocation precedes that
    /// simplification in MWCC.
    pub(crate) inline_source_call_survivors: HashSet<String>,
    /// Virtual homes corresponding to those pre-composition survivors. Their
    /// selected CFG may no longer cross a call, so allocation must exclude the
    /// volatile bank explicitly.
    pub(crate) forced_general_callee_saved: HashSet<VirtualRegister>,
    /// Frontend substitutions whose eliminated optimizer nodes still advance
    /// the anonymous ordinal stream when body expansion occurs.
    pub(crate) inline_expansion_facts: mwcc_syntax_trees::InlineExpansionFacts,
    /// Emit the saved-LR reload BEFORE the callee-saved GPR reloads in the epilogue. mwcc
    /// orders it this way for a callee-saved STORE sink (`foo(); gi = a;` — the saved value
    /// is stored after the call, then `lwz r0,20; lwz r31,12; mtlr`), as opposed to the
    /// return sink where the LR-reload hoist issues it right after the last call.
    pub(crate) epilogue_lr_first: bool,
    /// Emit the saved-LR reload BEFORE *all* callee-saved GPR reloads (highest-first), for a
    /// multi-pointer store sink: `void s(int*a,int*b){ *a=g(); *b=h(); }` saves both pointers
    /// (r31,r30), runs the calls, then `lwz r0,20; lwz r31,12; lwz r30,8; mtlr`. Distinct from
    /// `epilogue_lr_first`, whose two-GPR form interleaves the LR reload between the GPRs.
    pub(crate) epilogue_lr_before_gprs: bool,
    /// A whole-body owner emitted the measured LR save/reload placement itself.
    /// The generic latency passes must leave both ends of its frame untouched.
    pub(crate) owns_link_register_schedule: bool,
    /// Set while evaluating a narrow-return expression whose result is truncated, so a
    /// narrow leaf operand is read raw (no leading sign/zero extension) — the final
    /// truncation makes the extension redundant. Only enabled for truncation-safe
    /// operators with leaf operands, never for div/mod/shift-right.
    pub(crate) narrow_truncation_context: bool,
    /// The current function's declared local names — a CALL through one of these that
    /// never got a register must defer (the fallback would emit a direct `bl <local>`,
    /// a relocation against the local's name).
    pub(crate) known_locals: std::collections::HashSet<String>,
    /// C++ aggregate locals whose source-proven endpoint construction exposes
    /// their complete runtime representation as one word.
    pub(crate) one_word_aggregate_locals: std::collections::HashSet<String>,
    /// Narrow register locals whose every source assignment writes a complete
    /// 32-bit canonical boolean value (`0` or `1`).
    pub(crate) canonical_boolean_locals: std::collections::HashSet<String>,
    /// Large assertion strings whose address high halves remain live in saved
    /// registers across a structured loop.
    pub(crate) loop_assertion_string_highs: Vec<(Vec<u8>, u8)>,
    pub(crate) loop_assertion_string_highs_emitted: bool,
}

pub(crate) fn class_of(declared: Type) -> Compilation<ValueClass> {
    match declared {
        Type::Float | Type::Double => Ok(ValueClass::Float),
        Type::Void => Err(Diagnostic::error("a value cannot have type void")),
        _ => Ok(ValueClass::General),
    }
}

impl Generator {
    pub(crate) fn record_data_section_symbol_displacement(&mut self, symbol: &str) {
        self.output.data_section_displacements.push(
            mwcc_machine_code::DataSectionDisplacement {
                instruction_index: self.output.instructions.len(),
                target: mwcc_machine_code::DataSectionDisplacementTarget::Symbol(
                    symbol.to_owned(),
                ),
            },
        );
    }

    pub(crate) fn is_global_array(&self, name: &str) -> bool {
        self.global_arrays.contains(name)
    }

    /// Extent used only to select the symbol's address form. An unknown extern
    /// array is conservatively non-small; the sentinel never enters the finite
    /// extent map and therefore cannot satisfy bounds-based optimizations.
    pub(crate) fn global_array_address_extent(&self, name: &str) -> Option<u32> {
        self.is_global_array(name)
            .then(|| self.global_array_sizes.get(name).copied().unwrap_or(u32::MAX))
    }

    /// Signedness of a resolved syntax-tree type. The parser has already mapped
    /// a build-dependent plain `char` to either `Char` or `UnsignedChar`, while
    /// explicit `signed char` always remains `Char`. Codegen must therefore use
    /// the resolved type directly or build 53 incorrectly treats explicit
    /// signed characters as unsigned.
    pub(crate) fn signed_of(&self, declared: Type) -> bool {
        declared.is_signed()
    }

    /// A fresh general-purpose virtual register, as the u8 field value selection
    /// emits. The allocation pass resolves it to a physical register from liveness.
    pub(crate) fn fresh_virtual_general(&mut self) -> u8 {
        Reg::Virtual(self.virtual_cursors.next(Class::General)).to_field()
    }

    /// A fresh, unbound branch label. Branches emitted through
    /// [`Self::emit_branch_conditional_to`]/[`Self::emit_branch_to`] may target it
    /// before [`Self::bind_label`] pins where it lands; one resolve pass at the
    /// end of body emission writes every target.
    pub(crate) fn fresh_label(&mut self) -> mwcc_vreg::Label {
        self.labels.fresh()
    }

    /// Pin `label` to the next instruction to be emitted.
    pub(crate) fn bind_label(&mut self, label: mwcc_vreg::Label) {
        let at = self.output.instructions.len();
        self.labels.bind(label, at);
    }

    /// Emit a conditional branch to `label` (target written at resolution).
    pub(crate) fn emit_branch_conditional_to(
        &mut self,
        options: u8,
        condition_bit: u8,
        label: mwcc_vreg::Label,
    ) {
        self.labels.use_at(self.output.instructions.len(), label);
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options,
                condition_bit,
                target: 0,
            });
    }

    /// Emit an unconditional branch to `label` (target written at resolution).
    #[allow(dead_code)]
    pub(crate) fn emit_branch_to(&mut self, label: mwcc_vreg::Label) {
        self.labels.use_at(self.output.instructions.len(), label);
        self.output
            .instructions
            .push(Instruction::Branch { target: 0 });
    }

    /// A fresh floating-point virtual register. The allocator draws float homes
    /// from the FPR pool, kept distinct from the general pool by the class the
    /// machine description reports for each operand.
    pub(crate) fn fresh_virtual_float(&mut self) -> u8 {
        Reg::Virtual(self.virtual_cursors.next(Class::Float)).to_field()
    }

    /// A fresh floating virtual carrying the same consumer-placement preference
    /// used by general virtuals. Liveness still wins when the preferred FPR is
    /// occupied; otherwise this pins MWCC's short conversion schedules.
    pub(crate) fn fresh_virtual_float_preferring(&mut self, register: u8) -> u8 {
        let virtual_register = self.virtual_cursors.next(Class::Float);
        self.register_prefer.insert(virtual_register, register);
        Reg::Virtual(virtual_register).to_field()
    }

    /// Whether floating-point values in the current function already use the
    /// virtual-register allocator. New overlapping temporaries must join that
    /// allocation domain instead of being hard-pinned to a physical FPR.
    pub(crate) fn has_virtual_float_location(&self) -> bool {
        self.locations.values().any(|location| {
            location.class == ValueClass::Float && Reg::is_virtual_field(location.register)
        })
    }

    /// A fresh general virtual register carrying a consumer-tree PREFERENCE — the
    /// register the value's consumer wants it in (taken when free at allocation).
    pub(crate) fn fresh_virtual_general_preferring(&mut self, register: u8) -> u8 {
        let virtual_register = self.virtual_cursors.next(Class::General);
        self.register_prefer.insert(virtual_register, register);
        Reg::Virtual(virtual_register).to_field()
    }

    /// Update the allocator preference of an existing general virtual. Whole-body
    /// liveness passes use this when extending a temporary's live range changes
    /// MWCC's coloring order after instruction selection created the value.
    pub(crate) fn prefer_virtual_general(&mut self, register: u8, preferred: u8) {
        if let Reg::Virtual(register) = Reg::from_field(register, Class::General) {
            self.register_prefer.insert(register, preferred);
        }
    }

    /// Add a local consumer preference without replacing a whole-function
    /// lifetime/layout decision already attached to the value.
    pub(crate) fn prefer_virtual_general_if_unset(&mut self, register: u8, preferred: u8) {
        if let Reg::Virtual(register) = Reg::from_field(register, Class::General) {
            self.register_prefer.entry(register).or_insert(preferred);
        }
    }

    /// Prevent an existing general virtual from occupying any of `avoid`.
    /// Whole-body owners use this after they discover a retained value whose
    /// measured home must outrank otherwise independent locals.
    pub(crate) fn avoid_virtual_general(&mut self, register: u8, avoid: &[u8]) {
        if let Reg::Virtual(register) = Reg::from_field(register, Class::General) {
            let existing = self.register_avoid.entry(register).or_default();
            for avoided in avoid {
                if !existing.contains(avoided) {
                    existing.push(*avoided);
                }
            }
        }
    }

    /// A fresh general virtual register that the allocator must not place in any
    /// of `avoid` — a placement hint recorded for the allocation pass.
    pub(crate) fn fresh_virtual_general_avoiding(&mut self, avoid: Vec<u8>) -> u8 {
        let register = self.virtual_cursors.next(Class::General);
        self.register_avoid.insert(register, avoid);
        Reg::Virtual(register).to_field()
    }

    /// Restore a speculative selection attempt's virtual state. Every cache
    /// keyed by a virtual identity created by the discarded attempt must go
    /// too, or fallback emission can reuse the rolled-back ID without its
    /// definition (or inherit unrelated placement policy).
    pub(crate) fn rollback_virtuals(&mut self, checkpoint: VirtualCursors) {
        self.virtual_cursors = checkpoint;
        self.register_avoid
            .retain(|register, _| checkpoint.contains(*register));
        self.register_prefer
            .retain(|register, _| checkpoint.contains(*register));
        self.const_address_bases.retain(|_, register| {
            match Reg::from_field(*register, Class::General) {
                Reg::Virtual(register) => checkpoint.contains(register),
                Reg::Physical(_) => true,
            }
        });
    }

    /// Whether `expression` is a float-valued leaf.
    pub(crate) fn is_float_leaf(&self, expression: &Expression) -> bool {
        matches!(expression, Expression::Variable(name) if self.locations.get(name.as_str()).is_some_and(|l| l.class == ValueClass::Float))
    }

    /// See through a redundant `(double)` cast of an already-`double` value — a
    /// semantic no-op mwcc emits nothing for (`(double)dbl_call()`, `(double)dbl_x`).
    /// Peels every such layer, returning the innermost double operand. A `(float)`
    /// cast (a real narrowing) and a `(double)` of a non-double value are left intact.
    pub(crate) fn peel_redundant_double_cast<'a>(
        &self,
        mut expression: &'a Expression,
    ) -> &'a Expression {
        while let Expression::Cast {
            target_type: Type::Double,
            operand,
        } = expression
        {
            if self.is_double_value(operand) {
                expression = operand;
            } else {
                break;
            }
        }
        expression
    }

    /// Whether this expression yields a floating-point value — a float-register leaf,
    /// a float file-scope global, or a float-typed struct member — so a comparison on
    /// it routes to the FPU compare (`fcmpo`/`fcmpu`) path rather than the integer one.
    pub(crate) fn is_float_operand(&self, expression: &Expression) -> bool {
        match expression {
            Expression::FloatLiteral(_) => true,
            Expression::Variable(name) => {
                self.locations
                    .get(name.as_str())
                    .is_some_and(|location| location.class == ValueClass::Float)
                    || (!self.locations.contains_key(name.as_str())
                        && matches!(
                            self.globals.get(name.as_str()),
                            Some(Type::Float | Type::Double)
                        ))
            }
            Expression::Member { member_type, .. } => {
                matches!(member_type, Type::Float | Type::Double)
            }
            Expression::Dereference { pointer } => matches!(
                self.pointee_of(pointer),
                Ok(Pointee::Float | Pointee::Double)
            ),
            Expression::Index { base, .. } => matches!(
                self.pointee_of(base),
                Ok(Pointee::Float | Pointee::Double)
            ),
            Expression::Binary {
                operator,
                left,
                right,
            } if matches!(
                operator,
                BinaryOperator::Add
                    | BinaryOperator::Subtract
                    | BinaryOperator::Multiply
                    | BinaryOperator::Divide
            ) =>
            {
                self.is_float_operand(left) || self.is_float_operand(right)
            }
            Expression::Cast { target_type, .. } => {
                matches!(target_type, Type::Float | Type::Double)
            }
            Expression::Unary {
                operator: UnaryOperator::Negate,
                operand,
            } => self.is_float_operand(operand),
            Expression::Assign { target, value } => {
                self.is_float_operand(target) || self.is_float_operand(value)
            }
            Expression::Comma { right, .. } => self.is_float_operand(right),
            Expression::Conditional {
                when_true,
                when_false,
                ..
            } => self.is_float_operand(when_true) || self.is_float_operand(when_false),
            _ => false,
        }
    }

    /// Record a relocation against the instruction that is about to be pushed.
    pub(crate) fn record_relocation(&mut self, kind: RelocationKind, symbol: &str) {
        self.record_target(kind, RelocationTarget::External(symbol.to_string()));
    }
    /// Like [`Self::record_relocation`] but with a byte ADDEND — an SDA21
    /// load reading INTO a pooled object (strtold's `lbz r0, @53+0x4`).
    pub(crate) fn record_relocation_with_addend(
        &mut self,
        kind: RelocationKind,
        symbol: &str,
        addend: i32,
    ) {
        self.record_target(
            kind,
            RelocationTarget::ExternalWithAddend(symbol.to_string(), addend),
        );
    }

    /// Record a relocation with an explicit target (external symbol or pooled
    /// constant) against the instruction about to be pushed.
    pub(crate) fn record_target(&mut self, kind: RelocationKind, target: RelocationTarget) {
        let instruction_index = self.output.instructions.len();
        self.output.relocations.push(Relocation {
            instruction_index,
            kind,
            target,
        });
    }

    /// Select the base and relocation for one pooled-constant load. Absolute
    /// addressing emits the high-half materialization here; the caller owns
    /// the width-specific load and its returned low-half relocation.
    fn pooled_constant_load_address(&mut self, index: usize) -> (u8, RelocationKind) {
        if self.behavior.read_only_global_addressing == GlobalAddressing::SmallData {
            return (0, RelocationKind::EmbSda21);
        }
        let base = self.fresh_virtual_general();
        self.record_target(RelocationKind::Addr16Ha, RelocationTarget::Constant(index));
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: base,
                a: 0,
                immediate: 0,
            });
        (base, RelocationKind::Addr16Lo)
    }

    /// Emit a load of a single-precision pooled constant. Read-only small data
    /// uses one SDA21 load; `-sdata2 0` places the entry in `.rodata` and uses
    /// the measured `lis @ha; lfs @l(base)` pair.
    pub(crate) fn load_float_constant(&mut self, destination: u8, value: f32) {
        let index = self.output.intern_constant(value.to_bits() as u64, 4);
        let (base, relocation) = self.pooled_constant_load_address(index);
        self.record_target(relocation, RelocationTarget::Constant(index));
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: destination,
            a: base,
            offset: 0,
        });
    }

    /// Emit a load of an auto-array's pooled WORD IMAGE: like
    /// [`Self::load_word_constant`] but the entry numbers at the function's
    /// STATIC-LOCAL slot (measured: mbstring's first_byte_mark at `@4`).
    pub(crate) fn load_word_constant_static_slot(&mut self, destination: u8, bits: u32) {
        let index = self.output.intern_constant_static_slot(bits as u64, 4);
        let (base, relocation) = self.pooled_constant_load_address(index);
        self.record_target(relocation, RelocationTarget::Constant(index));
        self.output.instructions.push(Instruction::LoadWord {
            d: destination,
            a: base,
            offset: 0,
        });
    }

    /// Emit an auto-array image load that numbers in the POOL BLOCK but whose
    /// symbol leads the owning static function (ww's mbstring variant).
    pub(crate) fn load_word_constant_image(&mut self, destination: u8, bits: u32) {
        let index = self.output.intern_constant_image(bits as u64, 4);
        let (base, relocation) = self.pooled_constant_load_address(index);
        self.record_target(relocation, RelocationTarget::Constant(index));
        self.output.instructions.push(Instruction::LoadWord {
            d: destination,
            a: base,
            offset: 0,
        });
    }

    /// Emit a pooled word load using SDA21 or an absolute high/low pair.
    pub(crate) fn load_word_constant(&mut self, destination: u8, bits: u32) {
        let index = self.output.intern_constant(bits as u64, 4);
        let (base, relocation) = self.pooled_constant_load_address(index);
        self.record_target(relocation, RelocationTarget::Constant(index));
        self.output.instructions.push(Instruction::LoadWord {
            d: destination,
            a: base,
            offset: 0,
        });
    }

    /// Emit a pooled double load using SDA21 or an absolute high/low pair.
    pub(crate) fn load_double_constant(&mut self, destination: u8, bits: u64) {
        let index = self.output.intern_constant(bits, 8);
        let (base, relocation) = self.pooled_constant_load_address(index);
        self.record_target(relocation, RelocationTarget::Constant(index));
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: destination,
            a: base,
            offset: 0,
        });
    }

    /// Emit a pooled-double load against a SPECIFIC pool slot (a capture that
    /// interned twin slots for one value — strtold's zero doubles @296/@297).
    pub(crate) fn load_double_constant_at(&mut self, destination: u8, index: usize) {
        let (base, relocation) = self.pooled_constant_load_address(index);
        self.record_target(relocation, RelocationTarget::Constant(index));
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: destination,
            a: base,
            offset: 0,
        });
    }

    /// Load a float-literal operand, choosing 8-byte `lfd` in a double context and
    /// 4-byte `lfs` (the value rounded to single) otherwise.
    pub(crate) fn load_float_literal(&mut self, destination: u8, value: f64, double: bool) {
        if double {
            self.load_double_constant(destination, value.to_bits());
        } else {
            self.load_float_constant(destination, value as f32);
        }
    }

    pub(crate) fn lookup_general(&self, name: &str) -> Option<u8> {
        // Once a variable has addressable storage, its register is only an
        // incoming/initial value. A call through the escaped address may replace
        // the object, so all later value consumers must go through `evaluate`
        // and reload the frame slot rather than taking this fast register path.
        if self.frame_slots.contains_key(name) {
            return None;
        }
        self.locations
            .get(name)
            .filter(|location| location.class == ValueClass::General)
            .map(|location| location.register)
    }

    /// The register of a full-width, non-pointer integer leaf variable — the
    /// operand shape that participates in mwcc's additive-chain reassociation.
    /// Narrow leaves (which need width extension) and pointers (scaled
    /// arithmetic) return `None`.
    pub(crate) fn plain_integer_leaf_register(&self, expression: &Expression) -> Option<u8> {
        let name = leaf_name(expression)?;
        let location = self.locations.get(name)?;
        (location.class == ValueClass::General
            && location.width == 32
            && location.pointee.is_none())
        .then_some(location.register)
    }

    /// Whether `expression` is a narrow (sub-32-bit) integer variable. Such an
    /// operand needs width extension before use, and a few consumers (left shift
    /// and pow2 multiply) fuse extension and shift into a single `rlwinm` on the
    /// builds that treat `char` as unsigned — a peephole we do not model yet, so
    /// those callers defer narrow operands rather than emit non-matching bytes.
    pub(crate) fn is_narrow_leaf(&self, expression: &Expression) -> bool {
        matches!(expression, Expression::Variable(name)
            if self.locations.get(name.as_str()).is_some_and(|l| l.class == ValueClass::General && l.width < 32))
    }

    /// Width of an integer-like expression before the integral promotions.
    ///
    /// This is intentionally only the storage/value width needed to decide
    /// whether an unsigned operand promotes to `int`; the full expression type
    /// system remains in the syntax-tree layer.
    pub(crate) fn unpromoted_integer_width(&self, expression: &Expression) -> Option<u8> {
        match expression {
            Expression::Variable(name) => self
                .locations
                .get(name)
                .map(|location| location.width)
                .or_else(|| self.frame_slots.get(name).map(|slot| slot.value_type.width()))
                .or_else(|| self.globals.get(name).map(|value_type| value_type.width())),
            Expression::Member { member_type, .. } => Some(member_type.width()),
            Expression::Dereference { pointer } => {
                self.pointee_of(pointer).ok().map(|pointee| pointee.element().width())
            }
            Expression::Index { base, .. } => {
                self.pointee_of(base).ok().map(|pointee| pointee.element().width())
            }
            Expression::Cast { target_type, .. } => Some(target_type.width()),
            Expression::BitFieldRead { promoted_type, .. } => Some(promoted_type.width()),
            Expression::PostStep { target, .. } => self.unpromoted_integer_width(target),
            Expression::IndexedUpdateValue { value } => self.unpromoted_integer_width(value),
            // An assignment expression has the type of its left operand, not
            // the type of the value before conversion.
            Expression::Assign { target, .. } => self.unpromoted_integer_width(target),
            Expression::Comma { right, .. } => self.unpromoted_integer_width(right),
            Expression::VirtualCall { return_type, .. } => Some(return_type.width()),
            Expression::Call { name, .. } => self
                .call_return_types
                .get(name)
                .map(|return_type| return_type.width())
                .or(Some(32)),
            Expression::IntegerLiteral(_) | Expression::Unary { .. } | Expression::Binary { .. } => {
                Some(32)
            }
            _ => None,
        }
    }

    /// Signedness after C's integral promotions. Every 8- and 16-bit integer
    /// type fits in this target's 32-bit `int`, including their unsigned forms.
    fn promoted_integer_signedness_of(&self, expression: &Expression) -> Compilation<bool> {
        Ok(self.signedness_of(expression)?
            || self
                .unpromoted_integer_width(expression)
                .is_some_and(|width| width < 32))
    }

    /// Signedness after integral promotions and the usual arithmetic
    /// conversions for two integer operands.
    ///
    /// Narrow unsigned values promote to `int` on this 32-bit target, so
    /// `unsigned short` compared with `int` is a signed operation. When the
    /// promoted signs differ, a wider signed type can still represent every
    /// value of the narrower unsigned type; otherwise the common type is
    /// unsigned.
    pub(crate) fn usual_integer_binary_signedness(
        &self,
        left: &Expression,
        right: &Expression,
    ) -> Compilation<bool> {
        let promoted = |operand: &Expression| -> Compilation<(u8, bool)> {
            let width = self.unpromoted_integer_width(operand).unwrap_or(32).max(32);
            Ok((width, self.promoted_integer_signedness_of(operand)?))
        };
        let (left_width, left_signed) = promoted(left)?;
        let (right_width, right_signed) = promoted(right)?;
        if left_signed == right_signed {
            return Ok(left_signed);
        }
        if left_width == right_width {
            return Ok(false);
        }
        let (signed_width, unsigned_width) = if left_signed {
            (left_width, right_width)
        } else {
            (right_width, left_width)
        };
        Ok(signed_width > unsigned_width)
    }

    /// Whether the value of `expression` is signed (for selecting `>>`). The
    /// usual arithmetic conversions make a binary expression unsigned if either
    /// operand is unsigned.
    pub(crate) fn signedness_of(&self, expression: &Expression) -> Compilation<bool> {
        match expression {
            // A compound literal is a struct value — never a shift operand; treat signed.
            Expression::CompoundLiteral { .. } => Ok(true),
            // An indirect call's return type is unknown — signed by default,
            // like an unprototyped direct call.
            Expression::CallThrough { .. } => Ok(true),
            Expression::VirtualCall { return_type, .. } => Ok(self.signed_of(*return_type)),
            Expression::ConstructedNew { .. } => Ok(false),
            Expression::AggregateLiteral(_) => Err(Diagnostic::error(
                "an aggregate initializer is not supported here (captures only)",
            )),
            Expression::PostStep { target, .. } => self.signedness_of(target),
            Expression::IntegerLiteral(_) => Ok(true),
            Expression::FloatLiteral(_) => Ok(true),
            // A string literal is an address — an unsigned pointer value.
            Expression::StringLiteral(_) => Ok(false),
            Expression::Variable(name) => {
                if let Some(location) = self.locations.get(name) {
                    Ok(location.signed)
                } else if let Some(slot) = self.frame_slots.get(name) {
                    // An array expression decays to an unsigned address; a
                    // scalar retains the signedness of its declared type.
                    Ok(!slot.is_array && slot.value_type.is_signed())
                } else if let Some(global_type) = self.globals.get(name) {
                    Ok(global_type.is_signed())
                } else if let Some((_, element_type)) = self.fixed_address_arrays.get(name) {
                    Ok(element_type.is_signed())
                } else {
                    Err(Diagnostic::error(format!("unknown variable '{name}'")))
                }
            }
            Expression::Binary {
                operator,
                left,
                right,
            } => {
                if is_comparison(*operator) {
                    Ok(true) // a comparison yields an int (signed)
                } else if matches!(
                    operator,
                    BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr
                ) {
                    Ok(true) // logical operators yield int
                } else if matches!(
                    operator,
                    BinaryOperator::ShiftLeft | BinaryOperator::ShiftRight
                ) {
                    // Shift result type is the promoted type of the left operand;
                    // the right operand does not participate in its conversion.
                    self.promoted_integer_signedness_of(left)
                } else {
                    Ok(self.promoted_integer_signedness_of(left)?
                        && self.promoted_integer_signedness_of(right)?)
                }
            }
            Expression::Unary { operator, operand } => match operator {
                UnaryOperator::LogicalNot => Ok(true),
                _ => self.signedness_of(operand),
            },
            Expression::Conditional {
                when_true,
                when_false,
                ..
            } => Ok(self.signedness_of(when_true)? && self.signedness_of(when_false)?),
            Expression::Cast { target_type, .. } => Ok(self.signed_of(*target_type)),
            Expression::BitFieldRead { promoted_type, .. } => Ok(self.signed_of(*promoted_type)),
            Expression::IndexedUpdateValue { value } => self.signedness_of(value),
            // `*p` and `p[i]` have the signedness of the pointee.
            Expression::Dereference { pointer } => {
                Ok(self.pointee_of(pointer)?.element().is_signed())
            }
            Expression::Index { base, .. } => Ok(self.pointee_of(base)?.element().is_signed()),
            // `p->field` has the signedness of the member type.
            Expression::Member { member_type, .. } => Ok(self.signed_of(*member_type)),
            // An array member's address is an unsigned pointer.
            Expression::MemberAddress { .. } => Ok(false),
            // The address of an lvalue is an unsigned pointer.
            Expression::AddressOf { .. } => Ok(false),
            // An assignment yields the value after conversion to the left
            // operand's type.
            Expression::Assign { target, .. } => self.signedness_of(target),
            // A comma operator yields its right operand.
            Expression::Comma { right, .. } => self.signedness_of(right),
            // A call's signedness is its declared return type's; an unknown callee
            // defaults to a signed int.
            Expression::Call { name, .. } => Ok(self
                .call_return_types
                .get(name)
                .map_or(true, |return_type| self.signed_of(*return_type))),
        }
    }

    /// The pointee type of a pointer leaf variable.
    pub(crate) fn pointee_of(
        &self,
        pointer: &Expression,
    ) -> Compilation<mwcc_syntax_trees::Pointee> {
        // `*(p + i)` / `p[i]` of a pointer-plus-index dereferences the pointer operand's
        // pointee (the integer offset does not change the element type). `+` commutes. This
        // gives `signedness_of(*(p + i))` the element signedness, so `is_signed_byte_load`
        // recognizes a narrow `*(char* p + i)`.
        if let Expression::Binary {
            operator: mwcc_syntax_trees::BinaryOperator::Add,
            left,
            right,
        } = pointer
        {
            if let Ok(pointee) = self.pointee_of(left) {
                return Ok(pointee);
            }
            if let Ok(pointee) = self.pointee_of(right) {
                return Ok(pointee);
            }
        }
        // `*(T*)p` — a pointer cast reinterprets the address: the pointee is the cast's
        // target regardless of what `p` is (mirrors `resolve_pointer`, so value tracking
        // classifies a punned `*(int*)&x` the same way the direct evaluator emits it).
        if let Expression::Cast {
            target_type: Type::Pointer(pointee),
            ..
        } = pointer
        {
            return Ok(*pointee);
        }
        // `object->pointer_member[index]`: the pointer value is a structured
        // expression rather than a leaf variable, but its pointee is already
        // explicit in the member's resolved type.
        if let Expression::Member {
            member_type: Type::Pointer(pointee),
            ..
        } = pointer
        {
            return Ok(*pointee);
        }
        // An inline array member decays to a pointer whose element type is
        // explicit in the syntax tree (`object->values[index]`).
        if let Expression::MemberAddress { element, .. } = pointer {
            return Ok(*element);
        }
        // The first index of a flattened multidimensional frame array denotes
        // a row address. Its pointee remains the declared scalar element type;
        // the recorded row width affects addressing, not classification.
        if let Expression::Index { base, .. } = pointer {
            if let Expression::Variable(name) = base.as_ref() {
                if let Some(&pointee) = self.frame_row_pointees.get(name) {
                    return Ok(pointee);
                }
            }
        }
        let name = leaf_name(pointer).ok_or_else(|| {
            Diagnostic::error(format!(
                "pointer access needs a pointer variable (roadmap): {pointer:?}"
            ))
        })?;
        if let Some(pointee) = self
            .locations
            .get(name)
            .and_then(|location| location.pointee)
        {
            return Ok(pointee);
        }
        // A global ARRAY's name classifies by its element type (`map[i]` over
        // `unsigned char map[256]` reads a byte) — the subscript emitters carry
        // the addressing; this is only the width/signedness classification.
        if self.is_global_array(name) {
            if let Some(pointee) = self
                .globals
                .get(name)
                .copied()
                .and_then(crate::expressions::pointee_of_type)
            {
                return Ok(pointee);
            }
        }
        // Fixed-address hardware-register arrays carry their element type in a
        // separate addressing table rather than in emitted globals. Type queries
        // must still classify `bank[index]` before the specialized load/store
        // emitter materializes the absolute base.
        if let Some(pointee) = self
            .fixed_address_arrays
            .get(name)
            .and_then(|(_, element_type)| crate::expressions::pointee_of_type(*element_type))
        {
            return Ok(pointee);
        }
        Err(Diagnostic::error(format!("'{name}' is not a pointer")))
    }

    /// (register, width-bits, signed) for a general-register leaf variable.
    pub(crate) fn leaf_info(&self, expression: &Expression) -> Compilation<(u8, u8, bool)> {
        if let Expression::Variable(name) = expression {
            // An addressable object is no longer a register leaf. Its incoming
            // register may initialize the frame slot, but an escaped pointer can
            // replace the stored value before any later use.
            if self.frame_slots.contains_key(name) {
                return Err(Diagnostic::error("expected a general-register leaf"));
            }
            if let Some(location) = self.locations.get(name.as_str()) {
                if location.class == ValueClass::General {
                    return Ok((location.register, location.width, location.signed));
                }
            }
        }
        Err(Diagnostic::error("expected a general-register leaf"))
    }

    pub(crate) fn general_register_of(&self, name: &str) -> Compilation<u8> {
        let location = self
            .locations
            .get(name)
            .ok_or_else(|| Diagnostic::error(format!("unknown variable '{name}'")))?;
        if location.class != ValueClass::General {
            return Err(Diagnostic::error(format!("'{name}' is not an integer")));
        }
        Ok(location.register)
    }

    pub(crate) fn float_register_of(&self, name: &str) -> Compilation<u8> {
        let location = self
            .locations
            .get(name)
            .ok_or_else(|| Diagnostic::error(format!("unknown variable '{name}'")))?;
        if location.class != ValueClass::Float {
            return Err(Diagnostic::error(format!("'{name}' is not a float")));
        }
        Ok(location.register)
    }

    pub(crate) fn general_register_of_leaf(&self, expression: &Expression) -> Compilation<u8> {
        match expression {
            Expression::Variable(name) => self.general_register_of(name),
            _ => Err(Diagnostic::error(format!(
                "a general register was requested for a non-leaf expression: {expression:?}"
            ))),
        }
    }

    pub(crate) fn float_register_of_leaf(&self, expression: &Expression) -> Compilation<u8> {
        match expression {
            Expression::Variable(name) => self.float_register_of(name),
            _ => Err(Diagnostic::error(format!(
                "a float leaf operand must be a variable: {expression:?}"
            ))),
        }
    }

    /// Load a 32-bit integer constant the way mwcc does: `li`, or `lis` + `addi`
    /// with a high-adjusted upper half to absorb `addi`'s sign extension.
    pub(crate) fn load_integer_constant(&mut self, destination: u8, value: i64) {
        let value = value as i32;
        if (-0x8000..=0x7fff).contains(&value) {
            self.output
                .instructions
                .push(Instruction::load_immediate(destination, value as i16));
        } else {
            let low = (value as u32 & 0xffff) as i16;
            let high_adjusted = ((value - low as i32) >> 16) as i16;
            // The `addi` that folds in the low half reads `destination` as a base, but
            // `addi rA=r0` denotes the literal 0, not r0 — so materializing into r0
            // (the scratch) needs the `lis` in a separate register: `lis t,hi; addi
            // r0,t,lo` (mwcc colors `t` the lowest free GPR). Any other destination
            // folds in place.
            if destination == GENERAL_SCRATCH && low != 0 {
                let temp = self.fresh_virtual_general();
                self.output
                    .instructions
                    .push(Instruction::load_immediate_shifted(temp, high_adjusted));
                self.output.instructions.push(Instruction::AddImmediate {
                    d: destination,
                    a: temp,
                    immediate: low,
                });
            } else {
                self.output
                    .instructions
                    .push(Instruction::load_immediate_shifted(
                        destination,
                        high_adjusted,
                    ));
                // A constant whose low half is zero (`0x10000`, `0x80000000`) is a
                // single `lis`; mwcc omits the redundant `addi d,d,0`.
                if low != 0 {
                    self.output.instructions.push(Instruction::AddImmediate {
                        d: destination,
                        a: destination,
                        immediate: low,
                    });
                }
            }
        }
    }
}
