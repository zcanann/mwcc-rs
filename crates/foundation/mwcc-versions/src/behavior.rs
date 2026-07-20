//! Resolved codegen behavior: the single, inspectable set of decisions a
//! [`CompilerConfig`] (a build plus its flags) implies for the code generator.
//!
//! The pipeline never reaches into a build's profile or pokes at flags while
//! emitting code. It resolves one [`Behavior`] up front and reads named
//! decisions from it. The decisions that vary across builds are surfaced as
//! [`Quirk`]s, each carrying not just a value but *why* it exists — a deliberate
//! version-to-version design change, or the faithful reproduction of a real
//! compiler bug. That makes divergences enumerable: a configuration's active
//! quirks can be listed and explained ([`Behavior::active_quirks`]), so
//! reproducing a compiler bug is a deliberate, visible act rather than a magic
//! constant buried in instruction selection.

use crate::config::CompilerConfig;
use crate::flags::{GlobalAddressing, Optimization};
use crate::profile::{
    AsmBranchOptimizationStyle, AsmFunctionFinalizationStyle, BitFieldLoadPlacement,
    CoefficientTableRelocationStyle, CommaValuePlacementStyle, ComputedStoreIssueStyle,
    ConstantStoreScheduleStyle, FieldMergeStyle, FixedAddressConstantStoreStyle,
    FixedAddressParameterizedRmwStyle, FixedAddressPollAddressStyle, FixedAddressRmwStyle,
    FoldedFloatCompareLinkageStyle, FrameConvention, FrexpFamilyStyle,
    FunctionOrdinalAccountingStyle,
    DataSectionRelocationStyle, GlobalArrayDecayStoreStyle, GlobalArrayIndexStyle,
    IndexedRmwAssignmentStyle,
    IntCallResultConversionStyle,
    IntegerComparisonValueStyle, IntegerDagStyle, IntegerLoopStyle, IntegerSelectStyle,
    JumpTableBaseStyle, LeadingFrameGuardStoreStyle, LocalDataSymbolOrder, LogicalOrValueStyle,
    MaterializationCopyStyle, MemCopyRemainderMaskStyle, MemCopyWordScheduleStyle,
    NarrowCompoundShiftStyle, NarrowComputedReturnStyle, NarrowGuardScheduleStyle,
    NarrowStoreConversionStyle, NegativePowerOfTwoMultiplyStyle, PunnedConditionalWritebackStyle,
    PlainLinkageEpilogueStyle, PunnedFloatFrameConvention, PunnedShiftWritebackStyle,
    QueueServiceInliningStyle, RaiseFamilyStyle, ReadOnlySectionAnchorOrder,
    ReturnRegisterStoreStyle, SharedFloatDagStyle, SignedPowerOfTwoDivisionStyle,
    SmallZeroDataLayoutStyle, StoredGlobalReadStyle, SymbolTraversalStyle, TrigDispatcherStyle,
    TrigZeroConstantPlacement, VaArgScheduleStyle, ValueTrackedMutationStyle,
    WideConstantAddSchedule,
};

/// Why a codegen decision diverges from the GameCube 2.4.x mainline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuirkKind {
    /// A deliberate design change between versions — e.g. the plain-`char`
    /// signedness default that build 81 flipped back to signed.
    Intentional,
    /// A faithful reproduction of an actual compiler bug or accident: behavior
    /// that is "wrong" in isolation but must be matched to reproduce the
    /// original bytes. Kept distinct from [`QuirkKind::Intentional`] so bug
    /// emulation is always an explicit, documented choice.
    BugReproduction,
}

/// Lowering and scheduling applied to a NULL-terminated function-pointer-table
/// walker such as a REL module's `_prolog`/`_epilog` ctor/dtor loop.
///
/// This is selected by the invocation's optimization level rather than the
/// compiler build: GC/2.6 measurably exposes all four stages across `-O0`..`-O4`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerWalkerScheduleStyle {
    /// `-O0`: form the table address directly in its callee-saved home and
    /// independently load the entry in the loop body and condition.
    DirectAddressDuplicateLoad,
    /// `-O1`: stage the table address through r0, but retain the two source-level
    /// entry loads.
    ScratchAddressDuplicateLoad,
    /// `-O2`/`-O3`: reuse the condition's r12 entry load as the next indirect
    /// callee while keeping canonical linkage save/restore ordering.
    ReusedConditionLoad,
    /// `-O4`: additionally interleave address formation with linkage stores and
    /// issue the saved-LR reload early in the epilogue.
    LatencyInterleaved,
}

/// Placement of an absolute symbol's low relocation on a load or store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbsoluteAccessStyle {
    /// `-O0`: finish `lis`/`addi` address formation, then access offset zero.
    MaterializedAddress,
    /// `-O1` and above: fold the low relocation into the load/store displacement
    /// whenever the destination cannot double as the address base.
    FoldedDisplacement,
}

/// Evaluation schedule for `global * <wide integer constant>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalWideMultiplyStyle {
    /// `-O0`: load the global first, then materialize the constant in source order.
    Sequential,
    /// `-O1` and above: issue the constant high half ahead of the global load.
    Interleaved,
}

/// Whether an explicit source-level shift followed by a mask is combined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShiftMaskFusionStyle {
    /// `-O0`: preserve a shift instruction followed by a mask instruction.
    Separate,
    /// `-O1` and above: combine the pair into one rotate-and-mask instruction.
    Fused,
}

/// How a narrow call result is tested against zero in control flow.
///
/// GC/1.3.2 measurements across `-O0` through `-O4` put the peephole boundary
/// between O1 and O2: O0/O1 preserve the explicit width conversion and compare,
/// while O2+ fuse them into a record-form conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NarrowCallZeroTestStyle {
    /// `clrlwi r0,r3,24; cmplwi r0,0; bne ...`.
    SeparateCompare,
    /// `clrlwi. r0,r3,24; bne ...`.
    RecordConversion,
}

/// A named codegen decision that diverges from the mainline for some builds. The
/// set is closed (an enum) so every divergence has a stable identity that can be
/// listed, explained, and asserted against in tests. Each variant names the
/// *non-default* behavior; it is "active" exactly when a configuration exhibits
/// it (see [`Behavior::active_quirks`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quirk {
    /// Plain `char` (no `signed`/`unsigned` qualifier) defaults to *unsigned*
    /// rather than signed. GameCube build 53, and any `-char unsigned`. The
    /// mainline (build 81+) treats plain `char` as signed.
    UnsignedPlainChar,
    /// The int->float conversion stores the integer value (`stw rX,12(r1)`)
    /// before loading the bias double (`lfd f1,0(0)`), reversing the mainline
    /// schedule. Unique to GC/2.0p1.
    FloatCastStoresValueFirst,
    FloatCompareLoadsValueFirst,
    /// Build 163's int-to-float lowering stores a biased signed value through
    /// r0 before materializing the high word in that same register.
    LegacyFloatCastSchedule,
    LegacyFoldedFloatCompareBeforeLinkage,
    LegacyGuardHighBeforeLeadingFrameStore,
    LegacyFrexpPhysicalFrame,
    LegacyRaiseStagedLinkRegister,
    LegacyPortAwareIntegerDag,
    LegacyDependencyFirstIntegerLoops,
    LegacyBalancedSharedFloatDag,
    LegacyIntCallResultConversion,
    /// Build 163 preserves a compare/branch diamond for canonical integer
    /// boolean ternaries instead of using the 2.4.x branchless idioms.
    LegacyBranchPreservingIntegerSelect,
    LegacyCarryChainComparisonValues,
    LegacyFullWidthNarrowComputedReturn,
    LegacyCarryCorrectedPowerOfTwoDivision,
    LegacyEarlyInPlaceJumpTableBase,
    LegacyPartialNarrowStoreConversionElision,
    LegacyExplicitGlobalArrayAddress,
    LaterDirectGlobalArrayDecayStore,
    LegacyExplicitIndexedRmwAddress,
    LegacyReloadAfterGlobalStore,
    LegacyZeroEqualityNegate,
    LegacyReloadingPunnedFloatFrame,
    LegacyBranchPunnedConditionalWriteback,
    LegacyReloadingPunnedShiftWriteback,
    EarlyExpandedTrigDispatcherLabels,
    LegacyReloadingTrigDispatcher,
    EagerTrigZeroConstant,
    LaterTrigDispatcherLabels,
    LegacyAddImmediateMaterializationCopy,
    LegacySerialWideConstantAdd,
    LegacySymbolCreationOrder,
    LegacyLocalDataDeclarationOrder,
    LegacyForwardSmallZeroStatics,
    LegacyCoefficientTableSectionAnchor,
    EarlyDataSectionRelocationAnchors,
    LegacyMaterializedSectionPrototypes,
    LegacyEarlyReadOnlySectionAnchor,
    LegacyUnmarkedReadOnlySectionAnchor,
    LegacyUnmarkedSinglePrecisionExtab,
    LegacyZeroBasedInlineLocalStatics,
    LegacyZeroBaseStaticInlineLabels,
    LegacyInferredArrayFullDataSection,
    LegacyPreservedAsmBranchTargets,
    LegacyVerbatimAsmFrames,
    LegacyFixedAddressRmw,
    EarlyFoldedFixedPollDisplacement,
    LegacyFixedPollPageAddress,
    EarlyOutOfLineQueueService,
    LegacyOutOfLineQueueService,
    LegacyNarrowCompoundShift,
    LegacyTrueFirstLogicalOr,
    LegacyInterleavedConstantStores,
    LegacyEvaluationOrderComputedStores,
    LegacyInPlaceValueTrackedMutation,
    LegacyInPlaceNegativePowerOfTwoMultiply,
    LegacyLeftBaseFieldMerge,
    LegacyDelayedLeadingResultStore,
    LegacyCommaParameterHomes,
    LegacyInPlaceBitFieldExtraction,
    LegacyConstantJoinReturnBeforeLrReload,
    LegacyGuardStoreBeforeReturnValue,
    LegacyCompareFirstNarrowGuards,
    LegacySerialVaArgSchedule,
    Gc11PatchPlainLinkageReload,
    LaterTerminalIndirectTailCall,
}

impl Quirk {
    /// Whether this quirk is a deliberate version difference or a reproduced bug.
    pub fn kind(self) -> QuirkKind {
        match self {
            // Build 81 deliberately restored signed `char`; 53 is the older design.
            Quirk::UnsignedPlainChar => QuirkKind::Intentional,
            // A scheduling change introduced by the 2.0 patch release.
            Quirk::FloatCastStoresValueFirst => QuirkKind::Intentional,
            Quirk::FloatCompareLoadsValueFirst => QuirkKind::Intentional,
            Quirk::LegacyFloatCastSchedule => QuirkKind::Intentional,
            Quirk::LegacyFoldedFloatCompareBeforeLinkage => QuirkKind::Intentional,
            Quirk::LegacyGuardHighBeforeLeadingFrameStore => QuirkKind::Intentional,
            Quirk::LegacyFrexpPhysicalFrame => QuirkKind::Intentional,
            Quirk::LegacyRaiseStagedLinkRegister => QuirkKind::Intentional,
            Quirk::LegacyPortAwareIntegerDag => QuirkKind::Intentional,
            Quirk::LegacyDependencyFirstIntegerLoops => QuirkKind::Intentional,
            Quirk::LegacyBalancedSharedFloatDag => QuirkKind::Intentional,
            Quirk::LegacyIntCallResultConversion => QuirkKind::Intentional,
            Quirk::LegacyBranchPreservingIntegerSelect => QuirkKind::Intentional,
            Quirk::LegacyCarryChainComparisonValues => QuirkKind::Intentional,
            Quirk::LegacyFullWidthNarrowComputedReturn => QuirkKind::Intentional,
            Quirk::LegacyCarryCorrectedPowerOfTwoDivision => QuirkKind::Intentional,
            Quirk::LegacyEarlyInPlaceJumpTableBase => QuirkKind::Intentional,
            Quirk::LegacyPartialNarrowStoreConversionElision => QuirkKind::Intentional,
            Quirk::LegacyExplicitGlobalArrayAddress => QuirkKind::Intentional,
            Quirk::LaterDirectGlobalArrayDecayStore => QuirkKind::Intentional,
            Quirk::LegacyExplicitIndexedRmwAddress => QuirkKind::Intentional,
            Quirk::LegacyReloadAfterGlobalStore => QuirkKind::Intentional,
            Quirk::LegacyZeroEqualityNegate => QuirkKind::Intentional,
            Quirk::LegacyReloadingPunnedFloatFrame => QuirkKind::Intentional,
            Quirk::LegacyBranchPunnedConditionalWriteback => QuirkKind::Intentional,
            Quirk::LegacyReloadingPunnedShiftWriteback => QuirkKind::Intentional,
            Quirk::EarlyExpandedTrigDispatcherLabels => QuirkKind::Intentional,
            Quirk::LegacyReloadingTrigDispatcher => QuirkKind::Intentional,
            Quirk::EagerTrigZeroConstant => QuirkKind::Intentional,
            Quirk::LaterTrigDispatcherLabels => QuirkKind::Intentional,
            Quirk::LegacyAddImmediateMaterializationCopy => QuirkKind::Intentional,
            Quirk::LegacySerialWideConstantAdd => QuirkKind::Intentional,
            Quirk::LegacySymbolCreationOrder => QuirkKind::Intentional,
            Quirk::LegacyLocalDataDeclarationOrder => QuirkKind::Intentional,
            Quirk::LegacyForwardSmallZeroStatics => QuirkKind::Intentional,
            Quirk::LegacyCoefficientTableSectionAnchor => QuirkKind::Intentional,
            Quirk::EarlyDataSectionRelocationAnchors => QuirkKind::Intentional,
            Quirk::LegacyMaterializedSectionPrototypes => QuirkKind::Intentional,
            Quirk::LegacyEarlyReadOnlySectionAnchor => QuirkKind::Intentional,
            Quirk::LegacyUnmarkedReadOnlySectionAnchor => QuirkKind::Intentional,
            Quirk::LegacyUnmarkedSinglePrecisionExtab => QuirkKind::Intentional,
            Quirk::LegacyZeroBasedInlineLocalStatics => QuirkKind::Intentional,
            Quirk::LegacyZeroBaseStaticInlineLabels => QuirkKind::Intentional,
            Quirk::LegacyInferredArrayFullDataSection => QuirkKind::Intentional,
            Quirk::LegacyPreservedAsmBranchTargets => QuirkKind::Intentional,
            Quirk::LegacyVerbatimAsmFrames => QuirkKind::Intentional,
            Quirk::LegacyFixedAddressRmw => QuirkKind::Intentional,
            Quirk::EarlyFoldedFixedPollDisplacement => QuirkKind::Intentional,
            Quirk::LegacyFixedPollPageAddress => QuirkKind::Intentional,
            Quirk::EarlyOutOfLineQueueService => QuirkKind::Intentional,
            Quirk::LegacyOutOfLineQueueService => QuirkKind::Intentional,
            Quirk::LegacyNarrowCompoundShift => QuirkKind::Intentional,
            Quirk::LegacyTrueFirstLogicalOr => QuirkKind::Intentional,
            Quirk::LegacyInterleavedConstantStores => QuirkKind::Intentional,
            Quirk::LegacyEvaluationOrderComputedStores => QuirkKind::Intentional,
            Quirk::LegacyInPlaceValueTrackedMutation => QuirkKind::Intentional,
            Quirk::LegacyInPlaceNegativePowerOfTwoMultiply => QuirkKind::Intentional,
            Quirk::LegacyLeftBaseFieldMerge => QuirkKind::Intentional,
            Quirk::LegacyDelayedLeadingResultStore => QuirkKind::Intentional,
            Quirk::LegacyCommaParameterHomes => QuirkKind::Intentional,
            Quirk::LegacyInPlaceBitFieldExtraction => QuirkKind::Intentional,
            Quirk::LegacyConstantJoinReturnBeforeLrReload => QuirkKind::Intentional,
            Quirk::LegacyGuardStoreBeforeReturnValue => QuirkKind::Intentional,
            Quirk::LegacyCompareFirstNarrowGuards => QuirkKind::Intentional,
            Quirk::LegacySerialVaArgSchedule => QuirkKind::Intentional,
            Quirk::Gc11PatchPlainLinkageReload => QuirkKind::Intentional,
            Quirk::LaterTerminalIndirectTailCall => QuirkKind::Intentional,
        }
    }

    /// A one-line human explanation, for inspection and the artifact dump.
    pub fn summary(self) -> &'static str {
        match self {
            Quirk::UnsignedPlainChar => {
                "plain `char` defaults to unsigned (build 53 / -char unsigned)"
            }
            Quirk::FloatCastStoresValueFirst => {
                "int->float stores the value before loading the bias double (GC/2.0p1)"
            }
            Quirk::FloatCompareLoadsValueFirst => {
                "loaded float comparisons evaluate the value before the pool constant"
            }
            Quirk::LegacyFloatCastSchedule => {
                "int->float uses build 163's r0 scratch/store schedule"
            }
            Quirk::LegacyFoldedFloatCompareBeforeLinkage => {
                "folded float comparisons precede build 163's linkage instructions"
            }
            Quirk::LegacyGuardHighBeforeLeadingFrameStore => {
                "punned frame guards delay build 163's pointer store until the first guard-data use"
            }
            Quirk::LegacyFrexpPhysicalFrame => {
                "frexp uses build 163's padded physical writeback frame"
            }
            Quirk::LegacyRaiseStagedLinkRegister => {
                "raise stages its table load and dispatches through LR in build 163"
            }
            Quirk::LegacyPortAwareIntegerDag => {
                "integer DAGs use build 163's port-aware scheduler and serial r0 lane"
            }
            Quirk::LegacyDependencyFirstIntegerLoops => {
                "integer loops use build 163's compare-first entry, high temporary homes, and dependency-first schedule"
            }
            Quirk::LegacyBalancedSharedFloatDag => {
                "shared float DAGs use build 163's balanced prefix allocation and ready-op order"
            }
            Quirk::LegacyIntCallResultConversion => {
                "integer call results use build 163's bias-first conversion frame"
            }
            Quirk::LegacyBranchPreservingIntegerSelect => {
                "integer ternaries preserve build 163's source-level branch shape"
            }
            Quirk::LegacyCarryChainComparisonValues => {
                "integer comparison values use build 163's carry-chain idioms"
            }
            Quirk::LegacyFullWidthNarrowComputedReturn => {
                "computed narrow returns leave a full-width result in build 163"
            }
            Quirk::LegacyCarryCorrectedPowerOfTwoDivision => {
                "signed power-of-two division uses build 163's srawi/addze quotient"
            }
            Quirk::LegacyEarlyInPlaceJumpTableBase => {
                "jump tables finish their base in place before scaling the index in build 163"
            }
            Quirk::LegacyPartialNarrowStoreConversionElision => {
                "signed narrow stores preserve conversions outside build 163's binary-ALU fold set"
            }
            Quirk::LegacyExplicitGlobalArrayAddress => {
                "variable global-array indexes form an explicit address in build 163"
            }
            Quirk::LaterDirectGlobalArrayDecayStore => {
                "later compilers store a decayed global-array address directly from its address register"
            }
            Quirk::LegacyExplicitIndexedRmwAddress => {
                "explicit indexed read/modify/write assignments preserve an element address in build 163"
            }
            Quirk::LegacyReloadAfterGlobalStore => {
                "a read following a global store reloads memory in build 163"
            }
            Quirk::LegacyZeroEqualityNegate => {
                "zero equality negates into r0 before cntlzw in build 163"
            }
            Quirk::LegacyReloadingPunnedFloatFrame => {
                "punned float frames use build 163's padded, spill-reloading merge"
            }
            Quirk::LegacyBranchPunnedConditionalWriteback => {
                "conditional punned writebacks preserve build 163's branch diamond"
            }
            Quirk::LegacyReloadingPunnedShiftWriteback => {
                "shifted-mask punned writebacks use build 163's reload and allocation plan"
            }
            Quirk::EarlyExpandedTrigDispatcherLabels => {
                "deferred trigonometric dispatchers consume build 53's expanded anonymous-label block"
            }
            Quirk::LegacyReloadingTrigDispatcher => {
                "trigonometric dispatchers use build 163's linkage-first reload schedule"
            }
            Quirk::EagerTrigZeroConstant => {
                "later trigonometric dispatchers preload zero in the prologue"
            }
            Quirk::LaterTrigDispatcherLabels => {
                "later trigonometric dispatchers retain expanded hidden label blocks"
            }
            Quirk::LegacyAddImmediateMaterializationCopy => {
                "integer value materializations use build 163's add-immediate-zero copy encoding"
            }
            Quirk::LegacySerialWideConstantAdd => {
                "64-bit constant carry chains serialize their word constants through r0"
            }
            Quirk::LegacySymbolCreationOrder => {
                "symbols follow build 163 creation order across data, calls, and assignments"
            }
            Quirk::LegacyLocalDataDeclarationOrder => {
                "local data symbols preserve declaration order across initialized and zero sections"
            }
            Quirk::LegacyForwardSmallZeroStatics => {
                "file-scope static zero data is laid out first in declaration order"
            }
            Quirk::LegacyCoefficientTableSectionAnchor => {
                "coefficient-table bases relocate through the read-only section anchor"
            }
            Quirk::EarlyDataSectionRelocationAnchors => {
                "pointer initializers target full data-section anchors before build 81"
            }
            Quirk::LegacyMaterializedSectionPrototypes => {
                "section-attributed prototypes remain global undefined symbols in build 163"
            }
            Quirk::LegacyEarlyReadOnlySectionAnchor => {
                "the read-only section anchor precedes named data symbols"
            }
            Quirk::LegacyUnmarkedReadOnlySectionAnchor => {
                "the read-only section anchor carries no comment attribute flags"
            }
            Quirk::LegacyUnmarkedSinglePrecisionExtab => {
                "unsaved single-precision use leaves build 163's extab FPU bit clear"
            }
            Quirk::LegacyZeroBasedInlineLocalStatics => {
                "plain-inline static-local suffixes start at zero instead of three"
            }
            Quirk::LegacyZeroBaseStaticInlineLabels => {
                "dropped static-inline definitions have no base anonymous-label cost"
            }
            Quirk::LegacyInferredArrayFullDataSection => {
                "inferred-length arrays bypass build 163's small-data sections"
            }
            Quirk::LegacyPreservedAsmBranchTargets => {
                "asm branches preserve their written labels in build 163"
            }
            Quirk::LegacyVerbatimAsmFrames => {
                "asm frames stay verbatim and receive build 163's terminal return"
            }
            Quirk::LegacyFixedAddressRmw => {
                "fixed-address halfword updates use build 163's page base and promoted mask"
            }
            Quirk::EarlyFoldedFixedPollDisplacement => {
                "fixed-address polls fold the bank displacement into each build 53 load"
            }
            Quirk::LegacyFixedPollPageAddress => {
                "fixed-address polls materialize build 163's reusable bank page"
            }
            Quirk::EarlyOutOfLineQueueService => {
                "compound queue callers keep the service helper out of line in build 53"
            }
            Quirk::LegacyOutOfLineQueueService => {
                "compound queue callers keep the service helper out of line in build 163"
            }
            Quirk::LegacyNarrowCompoundShift => {
                "narrow compound shifts materialize build 163's count register"
            }
            Quirk::LegacyTrueFirstLogicalOr => "logical OR values use build 163's true-first exits",
            Quirk::LegacyInterleavedConstantStores => {
                "distinct constant-store runs use build 163's interleaved pair schedule"
            }
            Quirk::LegacyEvaluationOrderComputedStores => {
                "computed-store runs issue stores in build 163's value evaluation order"
            }
            Quirk::LegacyInPlaceValueTrackedMutation => {
                "straight-line mutable locals remain in build 163's result register"
            }
            Quirk::LegacyInPlaceNegativePowerOfTwoMultiply => {
                "negative power-of-two multiplies shift and negate in place in build 163"
            }
            Quirk::LegacyLeftBaseFieldMerge => {
                "field merges preserve build 163's masked left-operand base"
            }
            Quirk::LegacyDelayedLeadingResultStore => {
                "leaf store runs delay build 163's leading r3 result store by one slot"
            }
            Quirk::LegacyCommaParameterHomes => {
                "comma-operator values use build 163's parameter-home stack slots"
            }
            Quirk::LegacyInPlaceBitFieldExtraction => {
                "bit-field unit loads extract in place in build 163"
            }
            Quirk::LegacyConstantJoinReturnBeforeLrReload => {
                "constant non-leaf join returns precede build 163's link-register reload"
            }
            Quirk::LegacyGuardStoreBeforeReturnValue => {
                "guarded continuation stores precede build 163's return materialization"
            }
            Quirk::LegacyCompareFirstNarrowGuards => {
                "narrow guards use build 163's compare-first declaration-order schedule"
            }
            Quirk::LegacySerialVaArgSchedule => {
                "__va_arg ALIGN paths use build 163's serial r0 schedule"
            }
            Quirk::Gc11PatchPlainLinkageReload => {
                "GC/1.1p1 restores r1 before reloading LR from the caller linkage area"
            }
            Quirk::LaterTerminalIndirectTailCall => {
                "later compilers lower terminal indirect calls as unlinked sibling branches"
            }
        }
    }
}

/// The codegen decisions resolved from one [`CompilerConfig`]. This is the only
/// thing the code generator consults for version- and flag-varying behavior;
/// the build identity (version/build numbers, for object metadata) stays on the
/// config. Resolving once, here, keeps version checks out of instruction
/// selection — codegen reads a plain field, never a trait object or a flag.
#[derive(Debug, Clone, Copy)]
pub struct Behavior {
    /// Hidden labels retained around a call-dispatch jump table, separate from
    /// labels attributed to its individual arms.
    pub call_dispatcher_hidden_label_bump: u8,
    /// Hidden deferred-inlining labels retained per call-dispatch switch arm.
    /// Zero for ordinary compilation and for unmeasured compiler generations.
    pub deferred_call_dispatcher_labels_per_case: u8,
    /// Whether plain `char` is signed. Cascades through read/operand extension,
    /// `>>`/`/`/`%` strength reduction, comparison folding, and the int->float bias.
    pub char_is_signed: bool,
    /// In the int->float conversion, whether the value store is scheduled before
    /// the bias load (GC/2.0p1's order).
    pub float_cast_value_store_first: bool,
    /// Whether int-to-float uses build 163's r0 scratch/store ordering.
    pub legacy_float_cast_schedule: bool,
    /// Scheduling and frame family for integer call results converted to float.
    pub int_call_result_conversion_style: IntCallResultConversionStyle,
    /// In a non-leaf `if`-prologue, whether the saved-LR store precedes a leading
    /// float-constant load rather than filling the mflr->store latency slot with it
    /// (GC/2.0p1's order).
    pub lr_save_precedes_float_const: bool,
    /// Placement of a bare float comparison relative to non-leaf linkage when a
    /// following CR operation folds equality.
    pub folded_float_compare_linkage_style: FoldedFloatCompareLinkageStyle,
    /// Anonymous labels retained when a float guard folds to a branchless
    /// comparison value.
    pub folded_float_guard_label_bump: u8,
    /// Post-lowering anonymous-symbol accounting family.
    pub function_ordinal_accounting_style: FunctionOrdinalAccountingStyle,
    /// Scheduling of a leading pointer store around a punned frame guard.
    pub leading_frame_guard_store_style: LeadingFrameGuardStoreStyle,
    /// Whole-family schedule for the fdlibm-style `frexp` transaction.
    pub frexp_family_style: FrexpFamilyStyle,
    /// Additional anonymous labels retained around `frexp` when deferred
    /// inlining is enabled. Zero for ordinary compilation.
    pub frexp_deferred_label_bump: u8,
    /// Additional anonymous labels retained by `ldexp` control-flow graphs
    /// under deferred compilation. Zero for ordinary compilation.
    pub ldexp_deferred_label_bump: u8,
    /// Whole-family schedule for the signal-dispatch `raise` transaction.
    pub raise_family_style: RaiseFamilyStyle,
    /// Scheduler, register allocation, and symbol creation for integer DAGs.
    pub integer_dag_style: IntegerDagStyle,
    /// Entry, allocation, and scheduling policy for specialized integer loops.
    pub integer_loop_style: IntegerLoopStyle,
    /// Register allocation and issue order for MSL's aligned word-copy unroll.
    pub mem_copy_word_schedule_style: MemCopyWordScheduleStyle,
    /// Instruction selection for MSL's final three-byte remainder mask.
    pub mem_copy_remainder_mask_style: MemCopyRemainderMaskStyle,
    /// Whether the invocation runs mwcc's O4 latency scheduler over selected
    /// instructions and linkage save/reload slots.
    pub schedule_latency_slots: bool,
    /// Optimization-level policy for ctor/dtor function-pointer walkers.
    pub pointer_walker_schedule_style: PointerWalkerScheduleStyle,
    /// Optimization-level policy for absolute load/store displacement folding.
    pub absolute_access_style: AbsoluteAccessStyle,
    /// Optimization-level schedule for a global multiplied by a wide constant.
    pub global_wide_multiply_style: GlobalWideMultiplyStyle,
    /// Optimization-level policy for explicit shift/mask peephole fusion.
    pub shift_mask_fusion_style: ShiftMaskFusionStyle,
    /// Optimization-level peephole for a narrow call result tested against zero.
    pub narrow_call_zero_test_style: NarrowCallZeroTestStyle,
    /// Allocation and scheduling for float DAGs shared by two return arms.
    pub shared_float_dag_style: SharedFloatDagStyle,
    /// In a float `if`-condition against a pool constant, whether the loaded value
    /// operand (member/global) is emitted before the constant load.
    pub float_compare_value_before_const: bool,
    /// In `frexp`, whether the mantissa scaling `fmul` precedes the `*eptr` store
    /// (GC/2.0p1's order).
    pub frexp_scale_before_eptr_store: bool,
    /// Placement/order of the non-leaf linkage area.
    pub frame_convention: FrameConvention,
    /// Saved-LR reload order for a linkage-first frame with no saved GPRs.
    pub plain_linkage_epilogue_style: PlainLinkageEpilogueStyle,
    /// Whether stack-using leaf functions carry unwind-table entries.
    pub emit_leaf_frame_unwind: bool,
    /// Whether constant non-leaf join returns precede the saved-LR reload.
    pub constant_join_return_precedes_lr_reload: bool,
    /// Whether guarded continuation stores precede return-value materialization.
    pub guard_store_precedes_return_value: bool,
    /// Scheduling and local-home policy for narrow integer guard blocks.
    pub narrow_guard_schedule_style: NarrowGuardScheduleStyle,
    /// Scheduling policy for specialized `__va_arg` ALIGN paths.
    pub va_arg_schedule_style: VaArgScheduleStyle,
    /// Lowering of canonical integer boolean ternaries.
    pub integer_select_style: IntegerSelectStyle,
    /// Instruction family for integer comparisons materialized as 0/1 values.
    pub integer_comparison_value_style: IntegerComparisonValueStyle,
    /// Whether a computed same-width narrow return emits its explicit cast.
    pub narrow_computed_return_style: NarrowComputedReturnStyle,
    /// Lowering family for signed division/remainder by a power of two.
    pub signed_power_of_two_division_style: SignedPowerOfTwoDivisionStyle,
    /// Address-materialization schedule for switch jump tables.
    pub jump_table_base_style: JumpTableBaseStyle,
    /// Elimination policy for redundant signed narrow conversions before narrow stores.
    pub narrow_store_conversion_style: NarrowStoreConversionStyle,
    /// Placement of the containing-unit load for source-level bit-field reads.
    pub bit_field_load_placement: BitFieldLoadPlacement,
    /// Scheduling of distinct constant values consumed by consecutive stores.
    pub constant_store_schedule_style: ConstantStoreScheduleStyle,
    /// Issue order for stores fed by an overlapping two-value schedule.
    pub computed_store_issue_style: ComputedStoreIssueStyle,
    /// Placement of a returned local across source-level arithmetic reassignments.
    pub value_tracked_mutation_style: ValueTrackedMutationStyle,
    /// Placement of the shift in a negative power-of-two multiply.
    pub negative_power_of_two_multiply_style: NegativePowerOfTwoMultiplyStyle,
    /// Base orientation and redundant-mask policy for disjoint field merges.
    pub field_merge_style: FieldMergeStyle,
    /// Ordering of a leading store from the live r3 return value.
    pub return_register_store_style: ReturnRegisterStoreStyle,
    /// Placement of register parameters that survive a comma operator.
    pub comma_value_placement_style: CommaValuePlacementStyle,
    /// Addressing shape for variable-indexed file-scope arrays.
    pub global_array_index_style: GlobalArrayIndexStyle,
    /// Register placement for a bare array address stored to a pointer global.
    pub global_array_decay_store_style: GlobalArrayDecayStoreStyle,
    /// Addressing distinction between compound and explicit indexed RMW syntax.
    pub indexed_rmw_assignment_style: IndexedRmwAssignmentStyle,
    /// Treatment of an immediate read following a store to the same global.
    pub stored_global_read_style: StoredGlobalReadStyle,
    /// Whether zero equality negates its value into r0 before `cntlzw`.
    pub negate_before_zero_equality: bool,
    /// Frame/merge convention for type-punned floating parameters.
    pub punned_float_frame_convention: PunnedFloatFrameConvention,
    /// Additional labels retained by deferred punned-float else compositions.
    /// Zero for ordinary compilation.
    pub punned_float_composition_deferred_label_bump: u8,
    /// Lowering of conditional punned integer writebacks.
    pub punned_conditional_writeback_style: PunnedConditionalWritebackStyle,
    /// Frame, reload, and integer-allocation convention for shifted-mask writebacks.
    pub punned_shift_writeback_style: PunnedShiftWritebackStyle,
    /// Linkage and floating-spill schedule for trigonometric dispatchers.
    pub trig_dispatcher_style: TrigDispatcherStyle,
    /// Placement of the zero constant consumed by a dispatcher's small arm.
    pub trig_zero_constant_placement: TrigZeroConstantPlacement,
    /// Later-generation hidden labels retained by every trig dispatcher.
    pub trig_dispatcher_hidden_label_bump: u8,
    /// Additional hidden trig-dispatcher labels retained under whole-file IPA.
    pub trig_dispatcher_ipa_label_bump: u8,
    /// Encoding of generation-specific integer value materializations.
    pub materialization_copy_style: MaterializationCopyStyle,
    /// Scheduling of unequal constant words in a 64-bit add/subtract.
    pub wide_constant_add_schedule: WideConstantAddSchedule,
    /// AST traversal used to assign referenced symbol indices.
    pub symbol_traversal_style: SymbolTraversalStyle,
    /// Ordering of file-scope LOCAL data symbols across data sections.
    pub local_data_symbol_order: LocalDataSymbolOrder,
    /// Physical layout of `.sbss` objects.
    pub small_zero_data_layout_style: SmallZeroDataLayoutStyle,
    /// Relocation identity used by shared coefficient-table bases.
    pub coefficient_table_relocation_style: CoefficientTableRelocationStyle,
    /// Symbol-table placement of `...rodata.0` relative to named data.
    pub read_only_section_anchor_order: ReadOnlySectionAnchorOrder,
    /// `.comment` flags attached to the read-only section anchor.
    pub read_only_section_anchor_comment_flags: u32,
    /// Relocation identity used by pointer initializers targeting full data sections.
    pub data_section_relocation_style: DataSectionRelocationStyle,
    /// `.comment` flags attached to the writable `...data.0` section anchor.
    pub data_section_anchor_comment_flags: u32,
    /// Whether unused section-attributed prototypes remain in the symbol table.
    pub materialize_section_prototypes: bool,
    /// Whether unsaved single-precision use sets the extab FPU bit.
    pub mark_single_precision_extab: bool,
    /// First `$localstaticN` suffix within each plain inline definition.
    pub plain_inline_localstatic_base: u8,
    /// Base anonymous-label cost of a skipped static-inline definition.
    pub skipped_static_inline_label_base: u8,
    /// Version-specific weights applied to parser-recorded C++ inline facts.
    pub cxx_class_definition_label_bump: u8,
    pub cxx_inline_definition_label_bump: u8,
    pub cxx_virtual_destructor_label_bump: u8,
    pub cxx_inline_ipa_call_label_bump: u8,
    /// Whether initialized `T a[] = ...` objects bypass small-data routing.
    pub inferred_array_uses_full_data_section: bool,
    /// Post-resolution optimization of branches written in `asm` functions.
    pub asm_branch_optimization_style: AsmBranchOptimizationStyle,
    /// Frame wrapper and implicit-return policy for `asm` functions.
    pub asm_function_finalization_style: AsmFunctionFinalizationStyle,
    /// Base and mask selection for fixed-address halfword RMW leaves.
    pub fixed_address_rmw_style: FixedAddressRmwStyle,
    /// Register/schedule family for word fixed-address parameterized RMW leaves.
    pub fixed_address_parameterized_rmw_style: FixedAddressParameterizedRmwStyle,
    /// Base/value materialization order for fixed-address constant stores.
    pub fixed_address_constant_store_style: FixedAddressConstantStoreStyle,
    /// Address materialization used by fixed-register busy-wait loads.
    pub fixed_address_poll_address_style: FixedAddressPollAddressStyle,
    /// Whether verified compound queue callers inline the service helper CFG.
    pub queue_service_inlining_style: QueueServiceInliningStyle,
    /// Constant right-shift lowering for narrow global compound assignments.
    pub narrow_compound_shift_style: NarrowCompoundShiftStyle,
    /// Accumulator/exit convention for logical OR integer values.
    pub logical_or_value_style: LogicalOrValueStyle,
    /// How file-scope globals are addressed — small-data (SDA21 off r13) or
    /// absolute (ADDR16 hi/lo). Driven by `-sdata`; the resolved home for the
    /// addressing decision Phase C will consume.
    pub global_addressing: GlobalAddressing,
    /// How read-only file-scope objects are addressed — SDA2 (SDA21 off r2) or
    /// absolute (ADDR16 hi/lo). Driven independently by `-sdata2`.
    pub read_only_global_addressing: GlobalAddressing,
    /// Whether `-inline …,deferred` is active. Most deferred behavior belongs
    /// to TU/object orchestration; captures consult this only for measured
    /// codegen metadata differences.
    pub deferred_inlining: bool,
    /// Whether contiguous GPR saves/restores use inline `stmw`/`lmw` rather
    /// than `_savegpr_N`/`_restgpr_N` helper calls.
    pub use_lmw_stmw: bool,
    /// Whether independent instructions may fill one another's latency slots.
    pub scheduler_enabled: bool,
    /// Whether whole-file IPA may replace a terminal call/return with a sibling
    /// branch after marshaling its arguments.
    pub tail_call_optimization: bool,
    /// Whether the 4.x optimizer turns a terminal indirect call into an
    /// unlinked `bctr` sibling call without requiring whole-file IPA.
    pub terminal_indirect_tail_call: bool,
}

/// A quirk that is active for a configuration, paired with its kind and summary
/// so a caller can list and explain a build's divergences without re-deriving them.
#[derive(Debug, Clone, Copy)]
pub struct ActiveQuirk {
    pub quirk: Quirk,
    pub kind: QuirkKind,
    pub summary: &'static str,
}

impl ActiveQuirk {
    fn of(quirk: Quirk) -> Self {
        ActiveQuirk {
            quirk,
            kind: quirk.kind(),
            summary: quirk.summary(),
        }
    }
}

impl Behavior {
    /// Resolve every codegen decision for `config`, collapsing the build's
    /// profile and the flags into one flat set of values.
    pub fn resolve(config: &CompilerConfig) -> Self {
        Behavior {
            call_dispatcher_hidden_label_bump: config
                .build
                .profile
                .call_dispatcher_hidden_label_bump(),
            deferred_call_dispatcher_labels_per_case: if config.flags.inline_deferred {
                config
                    .build
                    .profile
                    .deferred_call_dispatcher_labels_per_case()
            } else {
                0
            },
            char_is_signed: config.char_is_signed(),
            float_cast_value_store_first: config.build.profile.float_cast_value_store_first(),
            legacy_float_cast_schedule: config.build.profile.legacy_float_cast_schedule(),
            int_call_result_conversion_style: config
                .build
                .profile
                .int_call_result_conversion_style(),
            lr_save_precedes_float_const: config.build.profile.lr_save_precedes_float_const(),
            folded_float_compare_linkage_style: config
                .build
                .profile
                .folded_float_compare_linkage_style(),
            folded_float_guard_label_bump: config.build.profile.folded_float_guard_label_bump()
                + if config.flags.ipa_file {
                    config.build.profile.folded_float_guard_ipa_label_bump()
                } else {
                    0
                },
            function_ordinal_accounting_style: match (
                config.build.profile.function_ordinal_accounting_style(),
                config.flags.ipa_file,
            ) {
                (FunctionOrdinalAccountingStyle::Gc41, true) => {
                    FunctionOrdinalAccountingStyle::Gc41Ipa
                }
                (style, _) => style,
            },
            leading_frame_guard_store_style: config.build.profile.leading_frame_guard_store_style(),
            frexp_family_style: config.build.profile.frexp_family_style(),
            frexp_deferred_label_bump: if config.flags.inline_deferred {
                config.build.profile.frexp_deferred_label_bump()
            } else {
                0
            },
            ldexp_deferred_label_bump: if config.flags.inline_deferred {
                config.build.profile.ldexp_deferred_label_bump()
            } else {
                0
            },
            raise_family_style: config.build.profile.raise_family_style(),
            integer_dag_style: config.build.profile.integer_dag_style(),
            integer_loop_style: config.build.profile.integer_loop_style(),
            mem_copy_word_schedule_style: config.build.profile.mem_copy_word_schedule_style(),
            mem_copy_remainder_mask_style: config.build.profile.mem_copy_remainder_mask_style(),
            schedule_latency_slots: config.flags.optimization == Optimization::O4,
            pointer_walker_schedule_style: match config.flags.optimization {
                Optimization::O0 => PointerWalkerScheduleStyle::DirectAddressDuplicateLoad,
                Optimization::O1 => PointerWalkerScheduleStyle::ScratchAddressDuplicateLoad,
                Optimization::O2 | Optimization::O3 => {
                    PointerWalkerScheduleStyle::ReusedConditionLoad
                }
                Optimization::O4 => PointerWalkerScheduleStyle::LatencyInterleaved,
            },
            absolute_access_style: if config.flags.optimization == Optimization::O0 {
                AbsoluteAccessStyle::MaterializedAddress
            } else {
                AbsoluteAccessStyle::FoldedDisplacement
            },
            global_wide_multiply_style: if config.flags.optimization == Optimization::O0 {
                GlobalWideMultiplyStyle::Sequential
            } else {
                GlobalWideMultiplyStyle::Interleaved
            },
            shift_mask_fusion_style: if config.flags.optimization == Optimization::O0 {
                ShiftMaskFusionStyle::Separate
            } else {
                ShiftMaskFusionStyle::Fused
            },
            narrow_call_zero_test_style: match config.flags.optimization {
                Optimization::O0 | Optimization::O1 => {
                    NarrowCallZeroTestStyle::SeparateCompare
                }
                Optimization::O2 | Optimization::O3 | Optimization::O4 => {
                    NarrowCallZeroTestStyle::RecordConversion
                }
            },
            shared_float_dag_style: config.build.profile.shared_float_dag_style(),
            float_compare_value_before_const: config
                .build
                .profile
                .float_compare_value_before_const(),
            frexp_scale_before_eptr_store: config.build.profile.frexp_scale_before_eptr_store(),
            frame_convention: config.build.profile.frame_convention(),
            plain_linkage_epilogue_style: config
                .build
                .profile
                .plain_linkage_epilogue_style(),
            emit_leaf_frame_unwind: config.build.profile.emit_leaf_frame_unwind(),
            constant_join_return_precedes_lr_reload: config
                .build
                .profile
                .constant_join_return_precedes_lr_reload(),
            guard_store_precedes_return_value: config
                .build
                .profile
                .guard_store_precedes_return_value(),
            narrow_guard_schedule_style: config.build.profile.narrow_guard_schedule_style(),
            va_arg_schedule_style: config.build.profile.va_arg_schedule_style(),
            integer_select_style: config.build.profile.integer_select_style(),
            integer_comparison_value_style: config.build.profile.integer_comparison_value_style(),
            narrow_computed_return_style: config.build.profile.narrow_computed_return_style(),
            signed_power_of_two_division_style: config
                .build
                .profile
                .signed_power_of_two_division_style(),
            jump_table_base_style: config.build.profile.jump_table_base_style(),
            narrow_store_conversion_style: config.build.profile.narrow_store_conversion_style(),
            bit_field_load_placement: config.build.profile.bit_field_load_placement(),
            constant_store_schedule_style: config.build.profile.constant_store_schedule_style(),
            computed_store_issue_style: config.build.profile.computed_store_issue_style(),
            value_tracked_mutation_style: config.build.profile.value_tracked_mutation_style(),
            negative_power_of_two_multiply_style: config
                .build
                .profile
                .negative_power_of_two_multiply_style(),
            field_merge_style: config.build.profile.field_merge_style(),
            return_register_store_style: config.build.profile.return_register_store_style(),
            comma_value_placement_style: config.build.profile.comma_value_placement_style(),
            global_array_index_style: config.build.profile.global_array_index_style(),
            global_array_decay_store_style: config
                .build
                .profile
                .global_array_decay_store_style(),
            indexed_rmw_assignment_style: config.build.profile.indexed_rmw_assignment_style(),
            stored_global_read_style: config.build.profile.stored_global_read_style(),
            negate_before_zero_equality: config.build.profile.negate_before_zero_equality(),
            punned_float_frame_convention: config.build.profile.punned_float_frame_convention(),
            punned_float_composition_deferred_label_bump: if config.flags.inline_deferred {
                config
                    .build
                    .profile
                    .punned_float_composition_deferred_label_bump()
            } else {
                0
            },
            punned_conditional_writeback_style: config
                .build
                .profile
                .punned_conditional_writeback_style(),
            punned_shift_writeback_style: config.build.profile.punned_shift_writeback_style(),
            trig_dispatcher_style: config.build.profile.trig_dispatcher_style(),
            trig_zero_constant_placement: config
                .build
                .profile
                .trig_zero_constant_placement(),
            trig_dispatcher_hidden_label_bump: config
                .build
                .profile
                .trig_dispatcher_hidden_label_bump(),
            trig_dispatcher_ipa_label_bump: config
                .build
                .profile
                .trig_dispatcher_ipa_label_bump(),
            materialization_copy_style: config.build.profile.materialization_copy_style(),
            wide_constant_add_schedule: config.build.profile.wide_constant_add_schedule(),
            symbol_traversal_style: config.build.profile.symbol_traversal_style(),
            local_data_symbol_order: config.build.profile.local_data_symbol_order(),
            small_zero_data_layout_style: config.build.profile.small_zero_data_layout_style(),
            coefficient_table_relocation_style: config
                .build
                .profile
                .coefficient_table_relocation_style(),
            read_only_section_anchor_order: config.build.profile.read_only_section_anchor_order(),
            read_only_section_anchor_comment_flags: config
                .build
                .profile
                .read_only_section_anchor_comment_flags(),
            data_section_relocation_style: config
                .build
                .profile
                .data_section_relocation_style(),
            data_section_anchor_comment_flags: config
                .build
                .profile
                .data_section_anchor_comment_flags(),
            materialize_section_prototypes: config
                .build
                .profile
                .materialize_section_prototypes(),
            mark_single_precision_extab: config.build.profile.mark_single_precision_extab(),
            plain_inline_localstatic_base: config.build.profile.plain_inline_localstatic_base(),
            skipped_static_inline_label_base: config
                .build
                .profile
                .skipped_static_inline_label_base(),
            cxx_class_definition_label_bump: config
                .build
                .profile
                .cxx_class_definition_label_bump(),
            cxx_inline_definition_label_bump: config
                .build
                .profile
                .cxx_inline_definition_label_bump(),
            cxx_virtual_destructor_label_bump: config
                .build
                .profile
                .cxx_virtual_destructor_label_bump(),
            cxx_inline_ipa_call_label_bump: if config.flags.ipa_file {
                config.build.profile.cxx_inline_ipa_call_label_bump()
            } else {
                0
            },
            inferred_array_uses_full_data_section: config
                .build
                .profile
                .inferred_array_uses_full_data_section(),
            asm_branch_optimization_style: config.build.profile.asm_branch_optimization_style(),
            asm_function_finalization_style: config.build.profile.asm_function_finalization_style(),
            fixed_address_rmw_style: config.build.profile.fixed_address_rmw_style(),
            fixed_address_parameterized_rmw_style: config
                .build
                .profile
                .fixed_address_parameterized_rmw_style(),
            fixed_address_constant_store_style: config
                .build
                .profile
                .fixed_address_constant_store_style(),
            fixed_address_poll_address_style: config
                .build
                .profile
                .fixed_address_poll_address_style(),
            queue_service_inlining_style: config.build.profile.queue_service_inlining_style(),
            narrow_compound_shift_style: config.build.profile.narrow_compound_shift_style(),
            logical_or_value_style: config.build.profile.logical_or_value_style(),
            global_addressing: config.flags.global_addressing,
            read_only_global_addressing: config.flags.read_only_global_addressing,
            deferred_inlining: config.flags.inline_deferred,
            use_lmw_stmw: config.flags.use_lmw_stmw,
            scheduler_enabled: config.flags.scheduler_enabled,
            tail_call_optimization: config.flags.ipa_file,
            terminal_indirect_tail_call: config
                .build
                .profile
                .terminal_indirect_tail_call(),
        }
    }

    /// The quirks that diverge from the mainline for this configuration, each
    /// with its kind and explanation. Empty for a plain mainline build — the
    /// list is exactly "what makes this configuration special".
    pub fn active_quirks(&self) -> Vec<ActiveQuirk> {
        let mut quirks = Vec::new();
        if !self.char_is_signed {
            quirks.push(ActiveQuirk::of(Quirk::UnsignedPlainChar));
        }
        if self.float_cast_value_store_first {
            quirks.push(ActiveQuirk::of(Quirk::FloatCastStoresValueFirst));
        }
        if self.float_compare_value_before_const {
            quirks.push(ActiveQuirk::of(Quirk::FloatCompareLoadsValueFirst));
        }
        if self.legacy_float_cast_schedule {
            quirks.push(ActiveQuirk::of(Quirk::LegacyFloatCastSchedule));
        }
        if self.folded_float_compare_linkage_style == FoldedFloatCompareLinkageStyle::CompareFirst {
            quirks.push(ActiveQuirk::of(
                Quirk::LegacyFoldedFloatCompareBeforeLinkage,
            ));
        }
        if self.leading_frame_guard_store_style
            == LeadingFrameGuardStoreStyle::GuardHighFirstAfterDataUse
        {
            quirks.push(ActiveQuirk::of(
                Quirk::LegacyGuardHighBeforeLeadingFrameStore,
            ));
        }
        if self.frexp_family_style == FrexpFamilyStyle::LegacyPhysicalFrame {
            quirks.push(ActiveQuirk::of(Quirk::LegacyFrexpPhysicalFrame));
        }
        if self.raise_family_style == RaiseFamilyStyle::StagedLoadLinkRegister {
            quirks.push(ActiveQuirk::of(Quirk::LegacyRaiseStagedLinkRegister));
        }
        if self.integer_dag_style == IntegerDagStyle::PortAwareSerialR0 {
            quirks.push(ActiveQuirk::of(Quirk::LegacyPortAwareIntegerDag));
        }
        if self.integer_loop_style == IntegerLoopStyle::LegacyDependencyFirst {
            quirks.push(ActiveQuirk::of(Quirk::LegacyDependencyFirstIntegerLoops));
        }
        if self.shared_float_dag_style == SharedFloatDagStyle::LegacyBalancedPrefix {
            quirks.push(ActiveQuirk::of(Quirk::LegacyBalancedSharedFloatDag));
        }
        if self.int_call_result_conversion_style == IntCallResultConversionStyle::LegacyBiasFirst {
            quirks.push(ActiveQuirk::of(Quirk::LegacyIntCallResultConversion));
        }
        if self.integer_select_style == IntegerSelectStyle::BranchPreserving {
            quirks.push(ActiveQuirk::of(Quirk::LegacyBranchPreservingIntegerSelect));
        }
        if self.integer_comparison_value_style == IntegerComparisonValueStyle::LegacyCarryChain {
            quirks.push(ActiveQuirk::of(Quirk::LegacyCarryChainComparisonValues));
        }
        if self.narrow_computed_return_style == NarrowComputedReturnStyle::FullWidthResult {
            quirks.push(ActiveQuirk::of(Quirk::LegacyFullWidthNarrowComputedReturn));
        }
        if self.signed_power_of_two_division_style
            == SignedPowerOfTwoDivisionStyle::CarryCorrectedQuotient
        {
            quirks.push(ActiveQuirk::of(
                Quirk::LegacyCarryCorrectedPowerOfTwoDivision,
            ));
        }
        if self.jump_table_base_style == JumpTableBaseStyle::EarlyInPlace {
            quirks.push(ActiveQuirk::of(Quirk::LegacyEarlyInPlaceJumpTableBase));
        }
        if self.narrow_store_conversion_style
            == NarrowStoreConversionStyle::PreserveOutsideBinaryAlu
        {
            quirks.push(ActiveQuirk::of(
                Quirk::LegacyPartialNarrowStoreConversionElision,
            ));
        }
        if self.bit_field_load_placement == BitFieldLoadPlacement::ResultRegister {
            quirks.push(ActiveQuirk::of(Quirk::LegacyInPlaceBitFieldExtraction));
        }
        if self.constant_store_schedule_style == ConstantStoreScheduleStyle::InterleavedPairs {
            quirks.push(ActiveQuirk::of(Quirk::LegacyInterleavedConstantStores));
        }
        if self.computed_store_issue_style == ComputedStoreIssueStyle::EvaluationOrder {
            quirks.push(ActiveQuirk::of(Quirk::LegacyEvaluationOrderComputedStores));
        }
        if self.value_tracked_mutation_style == ValueTrackedMutationStyle::InPlaceResultRegister {
            quirks.push(ActiveQuirk::of(Quirk::LegacyInPlaceValueTrackedMutation));
        }
        if self.negative_power_of_two_multiply_style
            == NegativePowerOfTwoMultiplyStyle::ShiftInResultRegister
        {
            quirks.push(ActiveQuirk::of(
                Quirk::LegacyInPlaceNegativePowerOfTwoMultiply,
            ));
        }
        if self.field_merge_style == FieldMergeStyle::LeftBasePreserveMask {
            quirks.push(ActiveQuirk::of(Quirk::LegacyLeftBaseFieldMerge));
        }
        if self.return_register_store_style
            == ReturnRegisterStoreStyle::DelayLeadingResultStoreOneSlot
        {
            quirks.push(ActiveQuirk::of(Quirk::LegacyDelayedLeadingResultStore));
        }
        if self.comma_value_placement_style == CommaValuePlacementStyle::ParameterHome {
            quirks.push(ActiveQuirk::of(Quirk::LegacyCommaParameterHomes));
        }
        if self.global_array_index_style == GlobalArrayIndexStyle::ExplicitAddress {
            quirks.push(ActiveQuirk::of(Quirk::LegacyExplicitGlobalArrayAddress));
        }
        if self.global_array_decay_store_style == GlobalArrayDecayStoreStyle::DirectAddress {
            quirks.push(ActiveQuirk::of(Quirk::LaterDirectGlobalArrayDecayStore));
        }
        if self.indexed_rmw_assignment_style == IndexedRmwAssignmentStyle::PreserveExplicitAddress {
            quirks.push(ActiveQuirk::of(Quirk::LegacyExplicitIndexedRmwAddress));
        }
        if self.stored_global_read_style == StoredGlobalReadStyle::ReloadAfterStore {
            quirks.push(ActiveQuirk::of(Quirk::LegacyReloadAfterGlobalStore));
        }
        if self.negate_before_zero_equality {
            quirks.push(ActiveQuirk::of(Quirk::LegacyZeroEqualityNegate));
        }
        if self.punned_float_frame_convention == PunnedFloatFrameConvention::LegacyReloading {
            quirks.push(ActiveQuirk::of(Quirk::LegacyReloadingPunnedFloatFrame));
        }
        if self.punned_conditional_writeback_style == PunnedConditionalWritebackStyle::BranchDiamond
        {
            quirks.push(ActiveQuirk::of(
                Quirk::LegacyBranchPunnedConditionalWriteback,
            ));
        }
        if self.punned_shift_writeback_style == PunnedShiftWritebackStyle::LegacyReloading {
            quirks.push(ActiveQuirk::of(Quirk::LegacyReloadingPunnedShiftWriteback));
        }
        if self.deferred_inlining
            && self.trig_dispatcher_style == TrigDispatcherStyle::EarlyLiveParameter
        {
            quirks.push(ActiveQuirk::of(Quirk::EarlyExpandedTrigDispatcherLabels));
        }
        if self.trig_dispatcher_style == TrigDispatcherStyle::LegacyReloading {
            quirks.push(ActiveQuirk::of(Quirk::LegacyReloadingTrigDispatcher));
        }
        if self.trig_zero_constant_placement == TrigZeroConstantPlacement::Prologue {
            quirks.push(ActiveQuirk::of(Quirk::EagerTrigZeroConstant));
        }
        if self.trig_dispatcher_hidden_label_bump != 0
            || self.trig_dispatcher_ipa_label_bump != 0
        {
            quirks.push(ActiveQuirk::of(Quirk::LaterTrigDispatcherLabels));
        }
        if self.materialization_copy_style == MaterializationCopyStyle::AddImmediateZero {
            quirks.push(ActiveQuirk::of(
                Quirk::LegacyAddImmediateMaterializationCopy,
            ));
        }
        if self.wide_constant_add_schedule == WideConstantAddSchedule::SerialScratchWords {
            quirks.push(ActiveQuirk::of(Quirk::LegacySerialWideConstantAdd));
        }
        if self.symbol_traversal_style == SymbolTraversalStyle::LegacyCreationOrder {
            quirks.push(ActiveQuirk::of(Quirk::LegacySymbolCreationOrder));
        }
        if self.local_data_symbol_order == LocalDataSymbolOrder::DeclarationOrder {
            quirks.push(ActiveQuirk::of(Quirk::LegacyLocalDataDeclarationOrder));
        }
        if self.small_zero_data_layout_style
            == SmallZeroDataLayoutStyle::LegacyStaticDeclarationOrderFirst
        {
            quirks.push(ActiveQuirk::of(Quirk::LegacyForwardSmallZeroStatics));
        }
        if self.coefficient_table_relocation_style
            == CoefficientTableRelocationStyle::SectionAnchorForComplexDag
        {
            quirks.push(ActiveQuirk::of(Quirk::LegacyCoefficientTableSectionAnchor));
        }
        if self.data_section_relocation_style == DataSectionRelocationStyle::SectionAnchor {
            quirks.push(ActiveQuirk::of(Quirk::EarlyDataSectionRelocationAnchors));
        }
        if self.materialize_section_prototypes {
            quirks.push(ActiveQuirk::of(
                Quirk::LegacyMaterializedSectionPrototypes,
            ));
        }
        if self.read_only_section_anchor_order == ReadOnlySectionAnchorOrder::BeforeDataObjects {
            quirks.push(ActiveQuirk::of(Quirk::LegacyEarlyReadOnlySectionAnchor));
        }
        if self.read_only_section_anchor_comment_flags == 0 {
            quirks.push(ActiveQuirk::of(Quirk::LegacyUnmarkedReadOnlySectionAnchor));
        }
        if !self.mark_single_precision_extab {
            quirks.push(ActiveQuirk::of(Quirk::LegacyUnmarkedSinglePrecisionExtab));
        }
        if self.plain_inline_localstatic_base == 0 {
            quirks.push(ActiveQuirk::of(Quirk::LegacyZeroBasedInlineLocalStatics));
        }
        if self.skipped_static_inline_label_base == 0 {
            quirks.push(ActiveQuirk::of(Quirk::LegacyZeroBaseStaticInlineLabels));
        }
        if self.inferred_array_uses_full_data_section {
            quirks.push(ActiveQuirk::of(Quirk::LegacyInferredArrayFullDataSection));
        }
        if self.asm_branch_optimization_style == AsmBranchOptimizationStyle::PreserveWrittenTargets
        {
            quirks.push(ActiveQuirk::of(Quirk::LegacyPreservedAsmBranchTargets));
        }
        if self.asm_function_finalization_style
            == AsmFunctionFinalizationStyle::VerbatimFrameWithTerminalReturn
        {
            quirks.push(ActiveQuirk::of(Quirk::LegacyVerbatimAsmFrames));
        }
        if self.fixed_address_rmw_style == FixedAddressRmwStyle::MaterializedPageWithPromotedMask {
            quirks.push(ActiveQuirk::of(Quirk::LegacyFixedAddressRmw));
        }
        match self.fixed_address_poll_address_style {
            FixedAddressPollAddressStyle::MaterializedElementForNonzeroIndex => {}
            FixedAddressPollAddressStyle::FoldedBankDisplacement => {
                quirks.push(ActiveQuirk::of(Quirk::EarlyFoldedFixedPollDisplacement))
            }
            FixedAddressPollAddressStyle::MaterializedBankPage => {
                quirks.push(ActiveQuirk::of(Quirk::LegacyFixedPollPageAddress));
            }
        }
        if self.queue_service_inlining_style == QueueServiceInliningStyle::KeepServiceCallOutOfLine
        {
            let quirk = if self.frame_convention == FrameConvention::LinkageFirst {
                Quirk::LegacyOutOfLineQueueService
            } else {
                Quirk::EarlyOutOfLineQueueService
            };
            quirks.push(ActiveQuirk::of(quirk));
        }
        if self.narrow_compound_shift_style == NarrowCompoundShiftStyle::MaterializedCount {
            quirks.push(ActiveQuirk::of(Quirk::LegacyNarrowCompoundShift));
        }
        if self.logical_or_value_style == LogicalOrValueStyle::TrueFirst {
            quirks.push(ActiveQuirk::of(Quirk::LegacyTrueFirstLogicalOr));
        }
        if self.constant_join_return_precedes_lr_reload {
            quirks.push(ActiveQuirk::of(
                Quirk::LegacyConstantJoinReturnBeforeLrReload,
            ));
        }
        if self.guard_store_precedes_return_value {
            quirks.push(ActiveQuirk::of(Quirk::LegacyGuardStoreBeforeReturnValue));
        }
        if self.narrow_guard_schedule_style
            == NarrowGuardScheduleStyle::CompareFirstDeclarationOrder
        {
            quirks.push(ActiveQuirk::of(Quirk::LegacyCompareFirstNarrowGuards));
        }
        if self.va_arg_schedule_style == VaArgScheduleStyle::SerialScratch {
            quirks.push(ActiveQuirk::of(Quirk::LegacySerialVaArgSchedule));
        }
        if self.plain_linkage_epilogue_style
            == PlainLinkageEpilogueStyle::StackRestoreBeforeReload
        {
            quirks.push(ActiveQuirk::of(Quirk::Gc11PatchPlainLinkageReload));
        }
        if self.terminal_indirect_tail_call {
            quirks.push(ActiveQuirk::of(Quirk::LaterTerminalIndirectTailCall));
        }
        quirks
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build, flags::CharDefault};

    #[test]
    fn optimization_level_selects_each_pointer_walker_schedule() {
        let expected = [
            (
                Optimization::O0,
                PointerWalkerScheduleStyle::DirectAddressDuplicateLoad,
                false,
                NarrowCallZeroTestStyle::SeparateCompare,
            ),
            (
                Optimization::O1,
                PointerWalkerScheduleStyle::ScratchAddressDuplicateLoad,
                false,
                NarrowCallZeroTestStyle::SeparateCompare,
            ),
            (
                Optimization::O2,
                PointerWalkerScheduleStyle::ReusedConditionLoad,
                false,
                NarrowCallZeroTestStyle::RecordConversion,
            ),
            (
                Optimization::O3,
                PointerWalkerScheduleStyle::ReusedConditionLoad,
                false,
                NarrowCallZeroTestStyle::RecordConversion,
            ),
            (
                Optimization::O4,
                PointerWalkerScheduleStyle::LatencyInterleaved,
                true,
                NarrowCallZeroTestStyle::RecordConversion,
            ),
        ];
        for (
            optimization,
            style,
            schedule_latency_slots,
            narrow_call_zero_test_style,
        ) in expected
        {
            let mut config = CompilerConfig::new(build::GC_2_6);
            config.flags.optimization = optimization;
            let behavior = Behavior::resolve(&config);
            assert_eq!(behavior.pointer_walker_schedule_style, style);
            assert_eq!(behavior.schedule_latency_slots, schedule_latency_slots);
            assert_eq!(
                behavior.narrow_call_zero_test_style,
                narrow_call_zero_test_style
            );
        }
    }

    #[test]
    fn o0_preserves_sequential_expression_lowering() {
        let mut config = CompilerConfig::new(build::GC_1_3_2);
        config.flags.optimization = Optimization::O0;
        let behavior = Behavior::resolve(&config);
        assert_eq!(
            behavior.absolute_access_style,
            AbsoluteAccessStyle::MaterializedAddress
        );
        assert_eq!(
            behavior.global_wide_multiply_style,
            GlobalWideMultiplyStyle::Sequential
        );
        assert_eq!(
            behavior.shift_mask_fusion_style,
            ShiftMaskFusionStyle::Separate
        );

        config.flags.optimization = Optimization::O1;
        let behavior = Behavior::resolve(&config);
        assert_eq!(
            behavior.absolute_access_style,
            AbsoluteAccessStyle::FoldedDisplacement
        );
        assert_eq!(
            behavior.global_wide_multiply_style,
            GlobalWideMultiplyStyle::Interleaved
        );
        assert_eq!(
            behavior.shift_mask_fusion_style,
            ShiftMaskFusionStyle::Fused
        );
    }

    #[test]
    fn msl_copy_policy_tracks_each_measured_generation_transition() {
        let early = Behavior::resolve(&CompilerConfig::new(build::GC_1_3));
        assert_eq!(
            early.mem_copy_word_schedule_style,
            MemCopyWordScheduleStyle::SerialScratch
        );
        assert_eq!(
            early.mem_copy_remainder_mask_style,
            MemCopyRemainderMaskStyle::MaterializedThree
        );

        let build_81 = Behavior::resolve(&CompilerConfig::new(build::GC_1_3_2));
        assert_eq!(
            build_81.mem_copy_word_schedule_style,
            MemCopyWordScheduleStyle::SerialScratch
        );
        assert_eq!(
            build_81.mem_copy_remainder_mask_style,
            MemCopyRemainderMaskStyle::FusedClearLeft
        );

        let build_92 = Behavior::resolve(&CompilerConfig::new(build::GC_2_0));
        assert_eq!(
            build_92.mem_copy_word_schedule_style,
            MemCopyWordScheduleStyle::PipelinedAlternatingScratch
        );
        assert_eq!(
            build_92.mem_copy_remainder_mask_style,
            MemCopyRemainderMaskStyle::FusedClearLeft
        );
    }

    #[test]
    fn mainline_has_no_active_quirks() {
        let behavior = Behavior::resolve(&CompilerConfig::new(build::GC_1_3_2));
        assert!(behavior.active_quirks().is_empty());
        assert!(behavior.char_is_signed);
    }

    #[test]
    fn build_53_reports_the_unsigned_char_quirk() {
        let behavior = Behavior::resolve(&CompilerConfig::new(build::GC_1_3));
        let quirks = behavior.active_quirks();
        assert_eq!(quirks.len(), 4);
        assert_eq!(quirks[0].quirk, Quirk::UnsignedPlainChar);
        assert_eq!(quirks[0].kind, QuirkKind::Intentional);
        assert_eq!(
            quirks[1].quirk,
            Quirk::EarlyDataSectionRelocationAnchors
        );
        assert_eq!(
            behavior.fixed_address_poll_address_style,
            FixedAddressPollAddressStyle::FoldedBankDisplacement
        );
        assert_eq!(quirks[2].quirk, Quirk::EarlyFoldedFixedPollDisplacement);
        assert_eq!(
            behavior.queue_service_inlining_style,
            QueueServiceInliningStyle::KeepServiceCallOutOfLine
        );
        assert_eq!(quirks[3].quirk, Quirk::EarlyOutOfLineQueueService);

        let mut deferred_config = CompilerConfig::new(build::GC_1_3);
        deferred_config.flags.inline_deferred = true;
        let deferred = Behavior::resolve(&deferred_config);
        assert_eq!(deferred.ldexp_deferred_label_bump, 20);
        assert_eq!(
            deferred.trig_dispatcher_style,
            TrigDispatcherStyle::EarlyLiveParameter
        );
        assert_eq!(
            deferred.active_quirks()[1].quirk,
            Quirk::EarlyExpandedTrigDispatcherLabels
        );
    }

    #[test]
    fn float_cast_quirk_is_unique_to_2_0p1() {
        let plain = Behavior::resolve(&CompilerConfig::new(build::GC_2_0));
        assert!(!plain.float_cast_value_store_first);
        let patched = Behavior::resolve(&CompilerConfig::new(build::GC_2_0P1));
        assert!(patched.float_cast_value_store_first);
        assert_eq!(
            patched.active_quirks()[0].quirk,
            Quirk::FloatCastStoresValueFirst
        );
    }

    #[test]
    fn char_flag_overrides_the_build_default_as_a_quirk() {
        let mut config = CompilerConfig::new(build::GC_1_3_2);
        config.flags.char_default = CharDefault::Unsigned;
        let behavior = Behavior::resolve(&config);
        assert!(!behavior.char_is_signed);
        assert_eq!(behavior.active_quirks()[0].quirk, Quirk::UnsignedPlainChar);
    }

    #[test]
    fn build_163_uses_linkage_first_frames() {
        let behavior = Behavior::resolve(&CompilerConfig::new(build::GC_1_2_5N));
        assert_eq!(behavior.ldexp_deferred_label_bump, 0);
        let mut deferred_config = CompilerConfig::new(build::GC_1_2_5N);
        deferred_config.flags.inline_deferred = true;
        assert_eq!(
            Behavior::resolve(&deferred_config).ldexp_deferred_label_bump,
            20
        );
        assert_eq!(behavior.frame_convention, FrameConvention::LinkageFirst);
        assert!(!behavior.emit_leaf_frame_unwind);
        assert!(behavior.constant_join_return_precedes_lr_reload);
        assert!(behavior.guard_store_precedes_return_value);
        assert!(behavior.materialize_section_prototypes);
        assert!(behavior
            .active_quirks()
            .iter()
            .any(|active| active.quirk == Quirk::LegacyMaterializedSectionPrototypes));
        assert_eq!(
            behavior.narrow_guard_schedule_style,
            NarrowGuardScheduleStyle::CompareFirstDeclarationOrder
        );
        assert_eq!(
            behavior.va_arg_schedule_style,
            VaArgScheduleStyle::SerialScratch
        );
        assert!(behavior.legacy_float_cast_schedule);
        assert_eq!(
            behavior.int_call_result_conversion_style,
            IntCallResultConversionStyle::LegacyBiasFirst
        );
        assert_eq!(
            behavior.integer_select_style,
            IntegerSelectStyle::BranchPreserving
        );
        assert_eq!(
            behavior.global_array_index_style,
            GlobalArrayIndexStyle::ExplicitAddress
        );
        assert_eq!(
            behavior.indexed_rmw_assignment_style,
            IndexedRmwAssignmentStyle::PreserveExplicitAddress
        );
        assert_eq!(
            behavior.stored_global_read_style,
            StoredGlobalReadStyle::ReloadAfterStore
        );
        assert!(behavior.negate_before_zero_equality);
        assert_eq!(
            behavior.punned_float_frame_convention,
            PunnedFloatFrameConvention::LegacyReloading
        );
        assert_eq!(
            behavior.punned_conditional_writeback_style,
            PunnedConditionalWritebackStyle::BranchDiamond
        );
        assert_eq!(
            behavior.punned_shift_writeback_style,
            PunnedShiftWritebackStyle::LegacyReloading
        );
        assert_eq!(
            behavior.trig_dispatcher_style,
            TrigDispatcherStyle::LegacyReloading
        );
        assert_eq!(
            behavior.materialization_copy_style,
            MaterializationCopyStyle::AddImmediateZero
        );
        assert_eq!(
            behavior.wide_constant_add_schedule,
            WideConstantAddSchedule::SerialScratchWords
        );
        assert_eq!(
            behavior.symbol_traversal_style,
            SymbolTraversalStyle::LegacyCreationOrder
        );
        assert!(!behavior.mark_single_precision_extab);
        assert_eq!(behavior.plain_inline_localstatic_base, 0);
        assert_eq!(
            behavior.asm_branch_optimization_style,
            AsmBranchOptimizationStyle::PreserveWrittenTargets
        );
        assert_eq!(
            behavior.asm_function_finalization_style,
            AsmFunctionFinalizationStyle::VerbatimFrameWithTerminalReturn
        );
        assert_eq!(
            behavior.fixed_address_rmw_style,
            FixedAddressRmwStyle::MaterializedPageWithPromotedMask
        );
        assert_eq!(
            behavior.fixed_address_poll_address_style,
            FixedAddressPollAddressStyle::MaterializedBankPage
        );
        assert_eq!(
            behavior.queue_service_inlining_style,
            QueueServiceInliningStyle::KeepServiceCallOutOfLine
        );
        assert_eq!(
            behavior.narrow_compound_shift_style,
            NarrowCompoundShiftStyle::MaterializedCount
        );
        assert_eq!(
            behavior.logical_or_value_style,
            LogicalOrValueStyle::TrueFirst
        );
        assert_eq!(
            behavior.computed_store_issue_style,
            ComputedStoreIssueStyle::EvaluationOrder
        );
        assert_eq!(
            behavior.value_tracked_mutation_style,
            ValueTrackedMutationStyle::InPlaceResultRegister
        );
        assert_eq!(
            behavior.negative_power_of_two_multiply_style,
            NegativePowerOfTwoMultiplyStyle::ShiftInResultRegister
        );
        assert_eq!(
            behavior.field_merge_style,
            FieldMergeStyle::LeftBasePreserveMask
        );
        assert_eq!(
            behavior.return_register_store_style,
            ReturnRegisterStoreStyle::DelayLeadingResultStoreOneSlot
        );
        assert_eq!(
            behavior.comma_value_placement_style,
            CommaValuePlacementStyle::ParameterHome
        );
        assert!(behavior.lr_save_precedes_float_const);
        assert_eq!(
            behavior.folded_float_compare_linkage_style,
            FoldedFloatCompareLinkageStyle::CompareFirst
        );
        assert_eq!(
            behavior.leading_frame_guard_store_style,
            LeadingFrameGuardStoreStyle::GuardHighFirstAfterDataUse
        );
        assert_eq!(
            behavior.frexp_family_style,
            FrexpFamilyStyle::LegacyPhysicalFrame
        );
        assert_eq!(
            behavior.raise_family_style,
            RaiseFamilyStyle::StagedLoadLinkRegister
        );
        assert_eq!(
            behavior.integer_dag_style,
            IntegerDagStyle::PortAwareSerialR0
        );
        assert_eq!(
            behavior.integer_loop_style,
            IntegerLoopStyle::LegacyDependencyFirst
        );
        assert_eq!(
            behavior.shared_float_dag_style,
            SharedFloatDagStyle::LegacyBalancedPrefix
        );
        assert!(behavior.float_compare_value_before_const);
        assert_eq!(
            behavior.bit_field_load_placement,
            BitFieldLoadPlacement::ResultRegister
        );
        assert!(Behavior::resolve(&CompilerConfig::new(build::GC_1_3_2)).emit_leaf_frame_unwind);
    }

    #[test]
    fn gc11_patch_restores_plain_linkage_stack_before_reloading_lr() {
        let original = Behavior::resolve(&CompilerConfig::new(build::GC_1_1));
        let patched = Behavior::resolve(&CompilerConfig::new(build::GC_1_1P1));
        let build_163 = Behavior::resolve(&CompilerConfig::new(build::GC_1_2_5));

        assert_eq!(
            original.plain_linkage_epilogue_style,
            PlainLinkageEpilogueStyle::ReloadBeforeStackRestore
        );
        assert_eq!(
            patched.plain_linkage_epilogue_style,
            PlainLinkageEpilogueStyle::StackRestoreBeforeReload
        );
        assert!(patched
            .active_quirks()
            .iter()
            .any(|active| active.quirk == Quirk::Gc11PatchPlainLinkageReload));
        assert_eq!(
            build_163.plain_linkage_epilogue_style,
            PlainLinkageEpilogueStyle::ReloadBeforeStackRestore
        );
    }

    #[test]
    fn lmw_stmw_flag_resolves_to_codegen_behavior() {
        let plain = Behavior::resolve(&CompilerConfig::new(build::GC_1_3));
        assert!(!plain.use_lmw_stmw);

        let mut config = CompilerConfig::new(build::GC_1_3);
        config.flags.use_lmw_stmw = true;
        assert!(Behavior::resolve(&config).use_lmw_stmw);
    }

    #[test]
    fn later_trig_dispatchers_resolve_eager_zero_and_ipa_labels() {
        for compiler_build in [build::GC_3_0A3P1, build::WII_1_0] {
            let mut config = CompilerConfig::new(compiler_build);
            let plain = Behavior::resolve(&config);
            assert_eq!(
                plain.global_array_decay_store_style,
                GlobalArrayDecayStoreStyle::DirectAddress
            );
            assert_eq!(
                plain.trig_zero_constant_placement,
                TrigZeroConstantPlacement::Prologue
            );
            assert_eq!(plain.trig_dispatcher_hidden_label_bump, 3);
            assert_eq!(plain.trig_dispatcher_ipa_label_bump, 4);
            assert!(plain.terminal_indirect_tail_call);
            assert!(plain
                .active_quirks()
                .iter()
                .any(|active| active.quirk == Quirk::EagerTrigZeroConstant));
            assert!(plain
                .active_quirks()
                .iter()
                .any(|active| active.quirk == Quirk::LaterTerminalIndirectTailCall));

            config.flags.ipa_file = true;
            let ipa = Behavior::resolve(&config);
            assert!(ipa.tail_call_optimization);
            assert_eq!(ipa.trig_dispatcher_ipa_label_bump, 4);
        }
    }

    #[test]
    fn gc41_coalesces_one_folded_float_guard_label() {
        let mainline = Behavior::resolve(&CompilerConfig::new(build::GC_2_7));
        let gc41 = Behavior::resolve(&CompilerConfig::new(build::GC_3_0A3P1));
        assert_eq!(mainline.folded_float_guard_label_bump, 2);
        assert_eq!(gc41.folded_float_guard_label_bump, 3);
        assert_eq!(
            gc41.function_ordinal_accounting_style,
            FunctionOrdinalAccountingStyle::Gc41
        );
        assert_eq!(mainline.cxx_inline_definition_label_bump, 0);
        assert_eq!(mainline.cxx_virtual_destructor_label_bump, 2);
        assert_eq!(gc41.cxx_class_definition_label_bump, 1);
        assert_eq!(gc41.cxx_inline_definition_label_bump, 4);
        assert_eq!(gc41.cxx_virtual_destructor_label_bump, 3);
        assert_eq!(gc41.cxx_inline_ipa_call_label_bump, 0);
        assert_eq!(mainline.call_dispatcher_hidden_label_bump, 0);
        assert_eq!(gc41.call_dispatcher_hidden_label_bump, 3);

        let mut gc41_deferred_config = CompilerConfig::new(build::GC_3_0A3P1);
        gc41_deferred_config.flags.inline_deferred = true;
        assert_eq!(
            Behavior::resolve(&gc41_deferred_config).deferred_call_dispatcher_labels_per_case,
            1
        );

        let mut wii_deferred_config = CompilerConfig::new(build::WII_1_0);
        wii_deferred_config.flags.inline_deferred = true;
        let wii_deferred = Behavior::resolve(&wii_deferred_config);
        assert_eq!(wii_deferred.call_dispatcher_hidden_label_bump, 0);
        assert_eq!(wii_deferred.deferred_call_dispatcher_labels_per_case, 1);
        assert!(wii_deferred_config
            .build
            .profile
            .prototype_parameter_names_consume_labels());

        let mut ipa_config = CompilerConfig::new(build::GC_3_0A3P1);
        ipa_config.flags.ipa_file = true;
        let ipa = Behavior::resolve(&ipa_config);
        assert_eq!(ipa.cxx_inline_ipa_call_label_bump, 1);
        assert_eq!(ipa.folded_float_guard_label_bump, 4);
        assert_eq!(
            ipa.function_ordinal_accounting_style,
            FunctionOrdinalAccountingStyle::Gc41Ipa
        );
    }

    #[test]
    fn schedule_off_disables_latency_slot_filling() {
        let mut config = CompilerConfig::new(build::GC_1_3_2);
        assert!(Behavior::resolve(&config).scheduler_enabled);
        config.flags.scheduler_enabled = false;
        assert!(!Behavior::resolve(&config).scheduler_enabled);
    }
}
