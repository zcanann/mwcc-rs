//! Per-build codegen behavior.
//!
//! Differences between mwcceppc builds are expressed as a [`CodegenProfile`]: a
//! trait whose default methods describe the GameCube 2.4.x mainline. A divergent
//! build overrides only the method that actually changed, so the shared behavior
//! — and every other build — stays untouched. Branching a new version off an
//! existing one is "add a profile struct, override one method", never a fork of
//! the whole code generator.

/// Placement of the linkage area relative to stack-pointer adjustment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameConvention {
    /// 2.4.x: decrement SP first; save LR above the new frame.
    Predecrement,
    /// 2.3.3: save LR through the incoming SP, then decrement SP.
    LinkageFirst,
}

/// Lowering used when an integer condition selects the canonical boolean
/// constants `1` and `0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegerSelectStyle {
    /// 2.4.x materializes the value with arithmetic/bit-count idioms.
    Branchless,
    /// 2.3.3 retains the compare-and-branch diamond from the source select.
    BranchPreserving,
}

/// Instruction family used when a relational/equality expression itself
/// materializes an integer 0/1 result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegerComparisonValueStyle {
    /// 2.4.x bitwise/count-leading-zero idioms.
    ModernBitwise,
    /// 2.3.3 carry-chain idioms built from `subfc`/`subfe`/`addze`.
    LegacyCarryChain,
}

/// Treatment of a computed integer expression returned from a narrow function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NarrowComputedReturnStyle {
    /// 2.4.x emits the final sign/zero extension in the callee.
    ExplicitNarrowing,
    /// 2.3.3 leaves the computed word in r3; the narrow return convention makes
    /// the low bits authoritative to the caller.
    FullWidthResult,
}

/// Lowering used for signed division and remainder by a positive power of two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignedPowerOfTwoDivisionStyle {
    /// 2.4.x uses its newer bias/mask idioms and range-optimizes promoted
    /// unsigned narrow operands as logical shifts.
    ModernRangeOptimized,
    /// 2.3.3 derives the rounded quotient from `srawi`'s carry with `addze`.
    /// Narrow operands are explicitly promoted into r0 first, including
    /// unsigned char/short values whose integer promotion makes them signed int.
    CarryCorrectedQuotient,
}

/// Address-materialization schedule for a switch jump table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpTableBaseStyle {
    /// 2.4.x scales the index before finishing the address, copying the table
    /// base into r3 for the indexed load.
    LateCopyToResultRegister,
    /// 2.3.3 finishes the address in its original register before scaling the
    /// index, and uses that register directly as the indexed-load base.
    EarlyInPlace,
}

/// Elimination policy for an explicit or implicit signed narrowing conversion
/// immediately consumed by a byte/halfword store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NarrowStoreConversionStyle {
    /// 2.4.x recognizes the store itself truncates and removes the conversion.
    ElideRedundantConversion,
    /// 2.3.3 removes the cast only after truncation-safe binary ALU operators;
    /// wider scalar/load/call, shift, unary, divide, and remainder operands keep
    /// both explicit casts and implicit assignment conversions.
    PreserveOutsideBinaryAlu,
}

/// Register used for the containing-unit load of a source-level bit-field read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitFieldLoadPlacement {
    /// 2.4.x loads a non-leaf bit-field unit through r0, then extracts into the
    /// requested result register.
    Scratch,
    /// 2.3.3 loads the unit directly into the result register and extracts in
    /// place. Ordinary explicit shift/mask expressions still use r0.
    ResultRegister,
}

/// Instruction shape used for a variable-indexed file-scope array element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalArrayIndexStyle {
    /// 2.4.x keeps base and scaled index separate for indexed load/store opcodes.
    Indexed,
    /// 2.3.3 forms one element address, then uses displacement zero.
    ExplicitAddress,
}

/// Whether an indexed read/modify/write preserves the frontend assignment form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexedRmwAssignmentStyle {
    /// 2.4.x selects indexed load/store instructions for both `a[i] op= x` and
    /// the equivalent explicitly spelled `a[i] = a[i] op x`.
    UniformIndexed,
    /// Build 163 forms an explicit element address only for the explicitly
    /// spelled assignment; compound assignment remains indexed.
    PreserveExplicitAddress,
}

/// Frame and merge policy for type-punned floating parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PunnedFloatFrameConvention {
    /// 2.4.x keeps the incoming FPR live across read-only guard merges.
    CompactLiveParameter,
    /// 2.3.3 reserves legacy top padding for materialized/writeback frames and
    /// reloads read-only fall-through values from their spill slot.
    LegacyReloading,
}

/// Encoding used for generation-specific integer value materializations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterializationCopyStyle {
    /// 2.4.x uses the canonical `mr` alias (`or d,s,s`).
    LogicalOr,
    /// 2.3.3 copies straight-line values through `addi d,s,0`; control-flow
    /// arm moves retain `mr`.
    AddImmediateZero,
}

/// Scheduling of unequal constant words in a 64-bit add/subtract carry chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WideConstantAddSchedule {
    /// Load low/high words into distinct registers before starting the chain.
    PreloadDistinctWords,
    /// Consume the low word from r0, then replace r0 with the high word before
    /// the carry-consuming `adde`.
    SerialScratchWords,
}

/// Scheduling of distinct constants consumed by a consecutive store run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstantStoreScheduleStyle {
    /// 2.4.x materializes every distinct value before emitting the stores.
    PreloadAll,
    /// 2.3.3 interleaves pairs of materializations with the earliest pending
    /// global store; pointer/member runs serialize through r0.
    InterleavedPairs,
}

/// AST traversal used to assign external/data symbol indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolTraversalStyle {
    /// Collect data references first, then call/function references.
    GroupedByKind,
    /// Preserve creation/evaluation order across symbol kinds; assignment
    /// values precede targets and data-only subtraction visits right-first.
    LegacyCreationOrder,
}

/// Ordering of file-scope LOCAL data symbols across initialized and zero-filled
/// sections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalDataSymbolOrder {
    /// Emit initialized objects forward, then uninitialized objects according to
    /// their reference/reverse-declaration rules.
    GroupedByInitialization,
    /// Preserve declaration order across `.sdata`/`.data` and `.sbss`/`.bss`.
    DeclarationOrder,
}

/// Physical layout of small zero-initialized data (`.sbss`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmallZeroDataLayoutStyle {
    /// Explicit-zero objects forward, followed by tentative definitions in
    /// reverse declaration order (the 2.4.x convention).
    ExplicitThenReverseTentative,
    /// Exported explicit-zero objects first, then file-scope statics in
    /// declaration order, then exported tentative definitions in reverse.
    LegacyStaticDeclarationOrderFirst,
}

/// Relocation identity used for a shared base into a read-only coefficient
/// table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoefficientTableRelocationStyle {
    /// Bind the ADDR16 pair directly to the table's LOCAL object symbol.
    NamedObject,
    /// Bind complex DAGs (two kept locals or at least three table loads) to the
    /// zero-offset `...rodata.0` section anchor.
    SectionAnchorForComplexDag,
}

/// Symbol-table placement of the synthetic `...rodata.0` section anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadOnlySectionAnchorOrder {
    /// Emit immediately after the first named `.rodata` object.
    AfterFirstObject,
    /// Emit before any named `.rodata` data symbol.
    BeforeDataObjects,
}

/// Optimizations applied after resolving labels in an `asm` function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsmBranchOptimizationStyle {
    /// 2.4.x chases unconditional branch chains and replaces branches whose
    /// final target is `blr` with the corresponding link-register form.
    ChaseAndCollapseReturns,
    /// 2.3.3 preserves the target written in the assembly source.
    PreserveWrittenTargets,
}

/// Frame and terminal-return handling for an `asm` function without
/// `nofralloc`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsmFunctionFinalizationStyle {
    /// 2.4.x wraps stack-using bodies in its generated frame and appends `blr`
    /// only when the written body falls through.
    GeneratedFrame,
    /// 2.3.3 leaves the written frame untouched and appends a terminal `blr`
    /// even when the written body already ends in a control transfer.
    VerbatimFrameWithTerminalReturn,
}

/// Base materialization and mask width for fixed-address halfword RMW leaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixedAddressRmwStyle {
    /// 2.4.x folds the bank low half into each displacement and range-narrows
    /// representable masks to the loaded halfword.
    FoldedDisplacementWithNarrowMask,
    /// 2.3.3 materializes the bank page for nonzero indexes and selects masks
    /// from the original promoted 32-bit expression.
    MaterializedPageWithPromotedMask,
}

/// Lowering of a constant right-shift compound assignment to a narrow global.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NarrowCompoundShiftStyle {
    /// 2.4.x loads into r0 and uses the immediate shift form.
    ImmediateInScratch,
    /// 2.3.3 loads into r3, materializes the count in r0, and uses `sraw`.
    MaterializedCount,
}

/// Accumulator/exit convention for a logical OR used as an integer value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalOrValueStyle {
    /// 2.4.x starts from false and materializes true at a shared taken path.
    FalseFirst,
    /// 2.3.3 starts from true and exits when either operand succeeds, falling
    /// through to materialize false only when both fail.
    TrueFirst,
}

/// The version-varying codegen decisions. Every method defaults to the GameCube
/// 2.4.x mainline (mwcceppc build 81 through 2.4.7 build 108); a build that
/// diverges implements this trait and overrides just the differing methods.
pub trait CodegenProfile: core::fmt::Debug {
    /// Whether plain `char` (no `signed`/`unsigned` qualifier) is signed. The one
    /// knob distinguishing GC build 53 from 81+; it cascades through read/operand
    /// extension, `>>`/`/`/`%` strength reduction, comparison folding, and the
    /// int->float bias.
    fn char_is_signed(&self) -> bool {
        true
    }

    /// In the int->float conversion, whether the value store (`stw rX,12(r1)`) is
    /// scheduled before the bias load (`lfd f1,0(0)`). GC/2.0p1 alone does this —
    /// the first observed instruction-scheduling difference between builds.
    fn float_cast_value_store_first(&self) -> bool {
        false
    }

    /// Build 163's older int-to-float schedule uses r0 for the biased signed
    /// value and stores the low word before reusing r0 for 0x43300000. Its
    /// unsigned form likewise stores the value before loading the bias pool.
    fn legacy_float_cast_schedule(&self) -> bool {
        false
    }

    /// In a non-leaf `if`-prologue, whether the saved-LR store (`stw r0,20(r1)`) is
    /// emitted BEFORE a leading float-constant load in the condition, rather than
    /// filling the `mflr`->store latency slot with that load. GC/2.0p1: `mflr r0;
    /// stw r0,20; lfs f0,0(0); fcmpo` vs mainline `mflr r0; lfs f0,0(0); stw r0,20;
    /// fcmpo`. The same "store before a float load" family as
    /// [`Self::float_cast_value_store_first`].
    fn lr_save_precedes_float_const(&self) -> bool {
        false
    }

    /// In a float `if`-condition comparing a LOADED value (member/global) against a
    /// pool CONSTANT, whether the value operand is loaded BEFORE the constant. GC/2.0p1:
    /// `lfs f1,(v); lfs f0,k` vs mainline `lfs f0,k; lfs f1,(v)` (which hoists the
    /// independent constant to fill the prologue latency slot). Same 2.0p1 float-reorder
    /// family; the register assignment (`fcmpo f1,f0`) is unchanged, only the load order.
    fn float_compare_value_before_const(&self) -> bool {
        false
    }

    /// In `frexp`, whether the mantissa scaling (`fmul`) is emitted before the
    /// `*eptr = <exp>` integer store. GC/2.0p1: `fmul; stw r0,0(r3)` vs mainline
    /// `stw r0,0(r3); fmul` — the two are independent, so it is purely a schedule
    /// difference. Same 2.0p1 float-reorder family.
    fn frexp_scale_before_eptr_store(&self) -> bool {
        false
    }

    /// Stack-frame linkage convention used by non-leaf functions.
    fn frame_convention(&self) -> FrameConvention {
        FrameConvention::Predecrement
    }

    /// Whether a leaf function that only allocates stack scratch receives
    /// extab/extabindex unwind metadata. The 2.4.x line catalogs these frames;
    /// build 163 omits them unless the function is non-leaf.
    fn emit_leaf_frame_unwind(&self) -> bool {
        true
    }

    /// In a non-leaf if/else join with a constant integer return, whether the
    /// return value is materialized before reloading the saved link register.
    fn constant_join_return_precedes_lr_reload(&self) -> bool {
        false
    }

    /// How integer `condition ? 1 : 0` (and its complement) is lowered.
    fn integer_select_style(&self) -> IntegerSelectStyle {
        IntegerSelectStyle::Branchless
    }

    fn integer_comparison_value_style(&self) -> IntegerComparisonValueStyle {
        IntegerComparisonValueStyle::ModernBitwise
    }

    fn narrow_computed_return_style(&self) -> NarrowComputedReturnStyle {
        NarrowComputedReturnStyle::ExplicitNarrowing
    }

    fn signed_power_of_two_division_style(&self) -> SignedPowerOfTwoDivisionStyle {
        SignedPowerOfTwoDivisionStyle::ModernRangeOptimized
    }

    fn jump_table_base_style(&self) -> JumpTableBaseStyle {
        JumpTableBaseStyle::LateCopyToResultRegister
    }

    fn narrow_store_conversion_style(&self) -> NarrowStoreConversionStyle {
        NarrowStoreConversionStyle::ElideRedundantConversion
    }

    fn bit_field_load_placement(&self) -> BitFieldLoadPlacement {
        BitFieldLoadPlacement::Scratch
    }

    fn constant_store_schedule_style(&self) -> ConstantStoreScheduleStyle {
        ConstantStoreScheduleStyle::PreloadAll
    }

    fn global_array_index_style(&self) -> GlobalArrayIndexStyle {
        GlobalArrayIndexStyle::Indexed
    }

    fn indexed_rmw_assignment_style(&self) -> IndexedRmwAssignmentStyle {
        IndexedRmwAssignmentStyle::UniformIndexed
    }

    /// Whether `value == 0` negates the value into r0 before `cntlzw`.
    /// This preserves build 163's older equality idiom for both register
    /// leaves and computed values.
    fn negate_before_zero_equality(&self) -> bool {
        false
    }

    fn punned_float_frame_convention(&self) -> PunnedFloatFrameConvention {
        PunnedFloatFrameConvention::CompactLiveParameter
    }

    fn materialization_copy_style(&self) -> MaterializationCopyStyle {
        MaterializationCopyStyle::LogicalOr
    }

    fn wide_constant_add_schedule(&self) -> WideConstantAddSchedule {
        WideConstantAddSchedule::PreloadDistinctWords
    }

    fn symbol_traversal_style(&self) -> SymbolTraversalStyle {
        SymbolTraversalStyle::GroupedByKind
    }

    fn local_data_symbol_order(&self) -> LocalDataSymbolOrder {
        LocalDataSymbolOrder::GroupedByInitialization
    }

    fn small_zero_data_layout_style(&self) -> SmallZeroDataLayoutStyle {
        SmallZeroDataLayoutStyle::ExplicitThenReverseTentative
    }

    fn coefficient_table_relocation_style(&self) -> CoefficientTableRelocationStyle {
        CoefficientTableRelocationStyle::NamedObject
    }

    fn read_only_section_anchor_order(&self) -> ReadOnlySectionAnchorOrder {
        ReadOnlySectionAnchorOrder::AfterFirstObject
    }

    /// `.comment` attribute flags attached to the `...rodata.0` symbol.
    fn read_only_section_anchor_comment_flags(&self) -> u32 {
        0x0010_0000
    }

    /// Whether unsaved single-precision use sets the extab FPU bit.
    fn mark_single_precision_extab(&self) -> bool {
        true
    }

    fn plain_inline_localstatic_base(&self) -> u8 {
        3
    }

    /// Base anonymous-label cost of compiling and dropping a `static inline`
    /// definition, before body-control-flow labels are counted.
    fn skipped_static_inline_label_base(&self) -> u8 {
        3
    }

    /// Whether an initialized array whose written length was inferred from `[]`
    /// bypasses the small-data size threshold. Build 163 places writable forms
    /// in `.data` and const forms in `.rodata`; the 2.4.x mainline uses the same
    /// size-based routing as explicitly sized arrays.
    fn inferred_array_uses_full_data_section(&self) -> bool {
        false
    }

    fn asm_branch_optimization_style(&self) -> AsmBranchOptimizationStyle {
        AsmBranchOptimizationStyle::ChaseAndCollapseReturns
    }

    fn asm_function_finalization_style(&self) -> AsmFunctionFinalizationStyle {
        AsmFunctionFinalizationStyle::GeneratedFrame
    }

    fn fixed_address_rmw_style(&self) -> FixedAddressRmwStyle {
        FixedAddressRmwStyle::FoldedDisplacementWithNarrowMask
    }

    fn narrow_compound_shift_style(&self) -> NarrowCompoundShiftStyle {
        NarrowCompoundShiftStyle::ImmediateInScratch
    }

    fn logical_or_value_style(&self) -> LogicalOrValueStyle {
        LogicalOrValueStyle::FalseFirst
    }
}

/// GameCube 2.4.x mainline — the reference behavior (all defaults). Covers
/// GC/1.3.2 (build 81), 1.3.2r, 2.0, 2.5, 2.6, and 2.7.
#[derive(Debug)]
pub struct Mainline;
impl CodegenProfile for Mainline {}

/// GC/1.3 — mwcceppc 2.4.2 build 53. The early 2.4.2 build that defaulted plain
/// `char` to unsigned, before build 81 restored signed.
#[derive(Debug)]
pub struct Gc13Build53;
impl CodegenProfile for Gc13Build53 {
    fn char_is_signed(&self) -> bool {
        false
    }
}

/// GC/1.2.5[n] — mwcceppc 2.3.3 build 163. Its first measured architectural
/// difference is the linkage-first stack frame; additional scheduler differences
/// remain under characterization, so this profile is experimental.
#[derive(Debug)]
pub struct Gc233Build163;
impl CodegenProfile for Gc233Build163 {
    fn frame_convention(&self) -> FrameConvention {
        FrameConvention::LinkageFirst
    }

    fn emit_leaf_frame_unwind(&self) -> bool {
        false
    }

    fn constant_join_return_precedes_lr_reload(&self) -> bool {
        true
    }

    fn legacy_float_cast_schedule(&self) -> bool {
        true
    }

    fn integer_select_style(&self) -> IntegerSelectStyle {
        IntegerSelectStyle::BranchPreserving
    }
    fn integer_comparison_value_style(&self) -> IntegerComparisonValueStyle {
        IntegerComparisonValueStyle::LegacyCarryChain
    }
    fn narrow_computed_return_style(&self) -> NarrowComputedReturnStyle {
        NarrowComputedReturnStyle::FullWidthResult
    }
    fn signed_power_of_two_division_style(&self) -> SignedPowerOfTwoDivisionStyle {
        SignedPowerOfTwoDivisionStyle::CarryCorrectedQuotient
    }
    fn jump_table_base_style(&self) -> JumpTableBaseStyle {
        JumpTableBaseStyle::EarlyInPlace
    }
    fn narrow_store_conversion_style(&self) -> NarrowStoreConversionStyle {
        NarrowStoreConversionStyle::PreserveOutsideBinaryAlu
    }
    fn bit_field_load_placement(&self) -> BitFieldLoadPlacement {
        BitFieldLoadPlacement::ResultRegister
    }
    fn constant_store_schedule_style(&self) -> ConstantStoreScheduleStyle {
        ConstantStoreScheduleStyle::InterleavedPairs
    }

    fn global_array_index_style(&self) -> GlobalArrayIndexStyle {
        GlobalArrayIndexStyle::ExplicitAddress
    }
    fn indexed_rmw_assignment_style(&self) -> IndexedRmwAssignmentStyle {
        IndexedRmwAssignmentStyle::PreserveExplicitAddress
    }
    fn negate_before_zero_equality(&self) -> bool {
        true
    }
    fn punned_float_frame_convention(&self) -> PunnedFloatFrameConvention {
        PunnedFloatFrameConvention::LegacyReloading
    }
    fn materialization_copy_style(&self) -> MaterializationCopyStyle {
        MaterializationCopyStyle::AddImmediateZero
    }
    fn wide_constant_add_schedule(&self) -> WideConstantAddSchedule {
        WideConstantAddSchedule::SerialScratchWords
    }
    fn symbol_traversal_style(&self) -> SymbolTraversalStyle {
        SymbolTraversalStyle::LegacyCreationOrder
    }
    fn local_data_symbol_order(&self) -> LocalDataSymbolOrder {
        LocalDataSymbolOrder::DeclarationOrder
    }
    fn small_zero_data_layout_style(&self) -> SmallZeroDataLayoutStyle {
        SmallZeroDataLayoutStyle::LegacyStaticDeclarationOrderFirst
    }
    fn coefficient_table_relocation_style(&self) -> CoefficientTableRelocationStyle {
        CoefficientTableRelocationStyle::SectionAnchorForComplexDag
    }
    fn read_only_section_anchor_order(&self) -> ReadOnlySectionAnchorOrder {
        ReadOnlySectionAnchorOrder::BeforeDataObjects
    }
    fn read_only_section_anchor_comment_flags(&self) -> u32 {
        0
    }
    fn mark_single_precision_extab(&self) -> bool {
        false
    }
    fn plain_inline_localstatic_base(&self) -> u8 {
        0
    }
    fn skipped_static_inline_label_base(&self) -> u8 {
        0
    }
    fn inferred_array_uses_full_data_section(&self) -> bool {
        true
    }

    fn asm_branch_optimization_style(&self) -> AsmBranchOptimizationStyle {
        AsmBranchOptimizationStyle::PreserveWrittenTargets
    }

    fn asm_function_finalization_style(&self) -> AsmFunctionFinalizationStyle {
        AsmFunctionFinalizationStyle::VerbatimFrameWithTerminalReturn
    }

    fn fixed_address_rmw_style(&self) -> FixedAddressRmwStyle {
        FixedAddressRmwStyle::MaterializedPageWithPromotedMask
    }

    fn narrow_compound_shift_style(&self) -> NarrowCompoundShiftStyle {
        NarrowCompoundShiftStyle::MaterializedCount
    }

    fn logical_or_value_style(&self) -> LogicalOrValueStyle {
        LogicalOrValueStyle::TrueFirst
    }
}

/// GC/2.0p1 — mwcceppc 2.4.7 build 92, patch 1. Mainline except it schedules the
/// int->float value store before the bias load.
#[derive(Debug)]
pub struct Gc20Patch1;
impl CodegenProfile for Gc20Patch1 {
    fn float_cast_value_store_first(&self) -> bool {
        true
    }
    fn lr_save_precedes_float_const(&self) -> bool {
        true
    }
    fn float_compare_value_before_const(&self) -> bool {
        true
    }
    fn frexp_scale_before_eptr_store(&self) -> bool {
        true
    }
}
