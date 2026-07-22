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

/// Prologue schedule for compiler-generated deleting destructors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CxxDestructorPrologueStyle {
    /// The 2.4.x scheduler compares `this` early and interleaves the two saved
    /// parameter homes with the callee-saved register stores.
    EarlyNullCheck,
    /// GC 4.1 saves both callee-saved registers, establishes their parameter
    /// homes, and only then compares `this` against null.
    SavedHomesBeforeNullCheck,
}

/// Ordering of the saved-LR reload in a linkage-first frame without saved GPRs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlainLinkageEpilogueStyle {
    /// Build 159 and build 163 reload through the decremented stack pointer,
    /// then restore r1: `lwz r0,frame+4(r1); addi r1,r1,frame`.
    ReloadBeforeStackRestore,
    /// GC/1.1p1 restores r1 first and reloads through the caller linkage area:
    /// `addi r1,r1,frame; lwz r0,4(r1)`.
    StackRestoreBeforeReload,
}

/// Placement of a bare floating-point comparison relative to non-leaf linkage
/// when a following `cror` folds equality for `<=` or `>=`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldedFloatCompareLinkageStyle {
    /// 2.4.x saves LR first, then emits the comparison into its latency slot.
    LinkRegisterFirst,
    /// Build 163 emits `fcmpo` before `mflr`, separating it from the dependent
    /// `cror` by the linkage instructions.
    CompareFirst,
}

/// Anonymous-symbol bookkeeping performed after ordinary function lowering.
/// This is object identity rather than instruction selection, but depends on
/// the optimized AST shape and therefore belongs beside codegen behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionOrdinalAccountingStyle {
    /// The 2.4.x generation's ordinary control-flow accounting is sufficient.
    Mainline,
    /// GC 4.1's non-IPA optimizer retains additional per-function nodes.
    Gc41,
    /// `-ipa file` changes which GC 4.1 nodes are charged before/after pools.
    Gc41Ipa,
}

/// Lowering family for SDK-style 64-bit stopwatch initialization and wait
/// transactions. These functions exercise paired integer values, volatile
/// pair spills, and EABI conversion helpers as one inseparable schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LongLongTimerStyle {
    /// GC 1.3 through 2.7 use the measured 2.4.x pair/register schedule.
    MainlinePair,
    /// This compiler generation has a different, not-yet-modeled schedule.
    Unmodeled,
}

/// Issue order for a nested-global callback whose outgoing arguments include a copied aggregate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NestedGlobalDispatchSchedule {
    /// Copy the three aggregate words together after linkage and argument-home setup.
    SequentialAggregateCopy,
    /// Issue each aggregate load in an earlier independent scheduler slot.
    EarlyAggregateLoads,
}

/// Schedule of a leading `*pointer = constant` around a punned frame guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeadingFrameGuardStoreStyle {
    /// 2.4.x materializes the store value before the guard high half and issues
    /// the store immediately after loading the guarded word.
    StoreValueFirstAfterLoad,
    /// Build 163 materializes the guard high half first and delays the store
    /// until the first guard-data operation has issued.
    GuardHighFirstAfterDataUse,
}

/// Materialization schedule for a null-guarded run of member stores that mixes
/// integer/pointer zero, floating zero, and incoming register values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardedMemberInitializationStyle {
    /// 2.3.3 materializes integer zero at the start, but delays the pooled float
    /// zero until immediately before its first store and then reuses it.
    LazyPooledFloat,
    /// 2.4.x materializes integer zero and pooled float zero before all stores.
    IntegerThenPooledFloat,
    /// GC/2.0p1 reloads the pooled float zero immediately before every float store.
    ReloadFloatPerStore,
    /// The 4.x compilers load pooled float zero before materializing integer zero.
    PooledFloatThenInteger,
}

/// Entry comparison and loop alignment for a null/zero-guarded byte copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardedByteCopyStyle {
    /// The 2.3.x and 2.4.x optimizers retain unsigned zero comparisons.
    LogicalCompare,
    /// GC 4.1 canonicalizes both equality tests to signed compares.
    SignedCompare,
    /// Wii 4.3 aligns the loop with one `nop` and issues its byte store before
    /// advancing the source cursor.
    SignedCompareWithAlignedStore,
}

/// Whole-family lowering of the fdlibm-style `frexp` transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrexpFamilyStyle {
    /// 2.4.x uses virtual local homes and its compact 16-byte punned frame.
    VirtualCompactFrame,
    /// Build 163 uses a padded physical frame with explicit lifetime-splitting
    /// copies around the writeback diamond.
    LegacyPhysicalFrame,
}

/// Whole-family lowering of the signal-dispatch `raise` transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RaiseFamilyStyle {
    /// 2.4.x loads the table entry directly into its callee-saved home and
    /// dispatches through CTR with logical-OR register copies.
    DirectLoadCountRegister,
    /// Build 163 stages the table entry through r0, completes the table address
    /// before scaling the index, and dispatches through LR with `addi` copies.
    StagedLoadLinkRegister,
}

/// Scheduling, register allocation, and symbol creation for straight-line
/// integer expression DAGs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegerDagStyle {
    /// 2.4.x uses the wide-issue scheduler and its closed-interval allocator;
    /// referenced symbols retain the source-AST traversal order.
    WideIssueClosedIntervals,
    /// Build 163 schedules distinct execution ports, stages the selected sink
    /// through a serial r0 lane, and creates symbols in scheduled-use order.
    PortAwareSerialR0,
}

/// Entry, allocation, and scheduling policy for specialized integer loops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegerLoopStyle {
    /// 2.4.x favors latency-filling schedules and reuses the dead CTR source home.
    ModernLatencyInterleaved,
    /// Build 163 evaluates the entry comparison first, keeps loop temporaries
    /// above the parameter homes, and completes dependency chains first.
    LegacyDependencyFirst,
}

/// Register allocation and issue order for the eight-word unroll in MSL's
/// aligned memory-copy helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemCopyWordScheduleStyle {
    /// Builds 53 and 81 keep one loaded word in r0 and complete each load/store
    /// pair before issuing the next load.
    SerialScratch,
    /// 2.4.7 and later alternate r3/r0 and issue the following load before the
    /// preceding store, hiding load latency.
    PipelinedAlternatingScratch,
}

/// Instruction selection for the final `n &= 3` in MSL memory-copy helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemCopyRemainderMaskStyle {
    /// Build 53 materializes `3` in r0 and uses `and.`.
    MaterializedThree,
    /// Other measured builds fuse the mask into `clrlwi.`.
    FusedClearLeft,
}

/// Allocation and ready-op ordering for a float DAG shared by both arms of
/// an integer-controlled return diamond.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharedFloatDagStyle {
    /// 2.4.x orders prefix homes by reverse definition and keeps coefficient
    /// loads ahead of newly ready shared-chain arithmetic.
    ModernDefinitionDescending,
    /// Build 163 rotates a three-product prefix to last/first/middle and lets
    /// newly ready shared-chain arithmetic precede an independent pool load.
    LegacyBalancedPrefix,
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

/// Register holding a file-scope array address assigned to a pointer global.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalArrayDecayStoreStyle {
    /// GC 1.x/2.x completes the address in r0 before storing it.
    ScratchValue,
    /// GC 3/Wii complete the address in its high-half register and store that
    /// register directly.
    DirectAddress,
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

/// Treatment of an immediate read following a store to the same file-scope
/// scalar. The stored register is semantically equivalent to reloading memory,
/// but the two compiler generations schedule different instruction streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredGlobalReadStyle {
    /// 2.4.x keeps the stored value live and feeds the following consumer from
    /// that register.
    ReuseStoredRegister,
    /// Build 163 reloads the global even when its just-stored value is still
    /// available in a register.
    ReloadAfterStore,
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

/// Lowering used for conditional punned integer writebacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PunnedConditionalWritebackStyle {
    /// Mainline optimizes constant diamonds through if-conversion or value hoisting.
    Optimized,
    /// Build 163 preserves the source-level branch diamond.
    BranchDiamond,
}

/// Frame, reload, and integer-allocation convention for shifted-mask punned writebacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PunnedShiftWritebackStyle {
    /// Mainline keeps the live floating parameter and allocates surviving homes before the mask.
    LiveParameter,
    /// Build 163 reloads the floating spill and allocates the shifted mask before the homes.
    LegacyReloading,
}

/// Linkage and floating-spill schedule for fdlibm trigonometric dispatchers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrigDispatcherStyle {
    /// Build 81 and later adjust the frame first and keep the incoming argument
    /// live. Dispatchers consume thirteen anonymous labels normally and
    /// nineteen under deferred compilation.
    LiveParameter,
    /// Build 53 uses the same instruction schedule but its earlier control-flow
    /// pass consumes twenty-four anonymous labels under deferred compilation.
    /// Non-deferred dispatchers retain the thirteen-label block.
    EarlyLiveParameter,
    /// Build 163 saves linkage through the incoming stack and reloads the
    /// argument spill. Its dispatcher label block consumes twelve entries
    /// normally and twenty-three under deferred compilation.
    LegacyReloading,
}

/// Placement of the dispatcher zero constant used by its small-argument arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrigZeroConstantPlacement {
    /// Load the constant after the range check, only on the small arm.
    SmallArm,
    /// Load the constant in the prologue before spilling the input argument.
    Prologue,
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

/// Store issue order after a two-value overlap schedule has materialized both
/// results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComputedStoreIssueStyle {
    /// 2.4.x issues the result expected to become ready first.
    ReadinessOrder,
    /// 2.3.3 preserves the value evaluation order.
    EvaluationOrder,
}

/// Placement of a source-level mutable local across a straight-line arithmetic
/// reassignment chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueTrackedMutationStyle {
    /// 2.4.x substitutes the local's expression and uses the ordinary expression
    /// allocator, including r0 for one-use intermediates.
    InlineExpression,
    /// 2.3.3 keeps a single returned local in r3 and mutates it in place at each
    /// source assignment boundary.
    InPlaceResultRegister,
}

/// Register used for the shift in `x * -2^N`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NegativePowerOfTwoMultiplyStyle {
    /// 2.4.x shifts into r0, then negates into the result register.
    ShiftThroughScratch,
    /// 2.3.3 shifts and negates in place in the result register.
    ShiftInResultRegister,
}

/// Base operand and redundant-mask policy for an OR of two disjoint fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldMergeStyle {
    /// 2.4.x computes the right operand as the `rlwimi` base and removes a mask
    /// when the inserted field overwrites every bit outside it.
    RightBaseElideCoveredMask,
    /// 2.3.3 masks the left operand as the base even when the other field covers
    /// the entire complement, then inserts the right operand.
    LeftBasePreserveMask,
}

/// Ordering of a leaf global-store run when the first value is already the r3
/// return value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReturnRegisterStoreStyle {
    /// 2.4.x keeps the source statement order.
    SourceOrder,
    /// 2.3.3 fills the first store slot from the next ready register, then emits
    /// the leading r3 store before continuing in source order.
    DelayLeadingResultStoreOneSlot,
}

/// Placement of register parameters that survive a comma operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommaValuePlacementStyle {
    /// 2.4.x keeps the surviving value in its incoming register.
    RegisterResident,
    /// 2.3.3 gives the surviving value an argument-home stack slot and reloads
    /// it at the consumer.
    ParameterHome,
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

/// Symbol identity used by pointer initializers targeting objects in the full
/// `.data` or `.rodata` sections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataSectionRelocationStyle {
    /// Bind directly to the named object symbol (build 81 and later).
    NamedObject,
    /// Bind to `...data.0` / `...rodata.0`, adding the object's section offset.
    SectionAnchor,
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

/// Register placement and scheduling for a word-sized fixed-address RMW that
/// inserts shifted parameter bits and returns a constant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixedAddressParameterizedRmwStyle {
    /// GC 2.4.x folds the bank low half into the load/store displacement.
    Mainline24,
    /// GC 2.3.3 materializes the complete bank address before the load.
    Legacy233,
    /// GC 1.3 build 53 materializes the preserve mask in a third register.
    Early24,
    /// The 4.x optimizer reverses the base/value registers and folds the
    /// constant insert into the loaded value rather than the shifted parameter.
    Modern4x,
}

/// Scheduling of the base and value materializations for a constant written to
/// a constant-index fixed-address array slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixedAddressConstantStoreStyle {
    /// Mainline 2.4.x and GC 4.1 materialize the r0 value before the base.
    ValueFirst,
    /// GC 2.3.3 and Wii 4.3 materialize the fixed base before the r0 value.
    BaseFirst,
}

/// Address shape used by a busy-wait load from a fixed-address register bank.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixedAddressPollAddressStyle {
    /// Build 81 and later materialize a nonzero element's full address, then
    /// poll it at displacement zero. Element zero retains the bank low half as
    /// the load displacement.
    MaterializedElementForNonzeroIndex,
    /// Build 53 keeps the bank high half as the base and folds the bank low
    /// half plus the element offset into the polling load.
    FoldedBankDisplacement,
    /// Build 163 materializes the bank page and keeps only the element offset
    /// in the polling load.
    MaterializedBankPage,
}

/// Interprocedural policy for a verified chunked queue-service helper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueServiceInliningStyle {
    /// Build 81 and later splice the service helper CFG into its interrupt and
    /// queue-posting callers.
    InlineVerifiedCallers,
    /// Builds 53 and 163 recognize the helper as an inline candidate but leave
    /// the service call out of line in these compound callers.
    KeepServiceCallOutOfLine,
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

/// Scheduling and local-home policy for narrow integer guard blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NarrowGuardScheduleStyle {
    /// 2.4.x fills the width-op/compare latency gap and chooses homes from the
    /// return expression's consumer tree.
    LatencyInterleavedConsumerTree,
    /// Build 163 compares first and assigns single-block homes in declaration
    /// order beginning with r0, then r3 and the remaining volatile registers.
    CompareFirstDeclarationOrder,
}

/// Instruction ordering used by the specialized `__va_arg` ALIGN schedules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaArgScheduleStyle {
    /// 2.4.x fills independent latency slots with store/address computations.
    LatencyInterleaved,
    /// Build 163 serializes the byte-store constant through r0 and preserves
    /// the address-before-store-value order in the register-counter arm.
    SerialScratch,
}

/// Scheduling and frame family for an integer call result converted to float.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntCallResultConversionStyle {
    /// 2.4.x follows the call-result value chain before loading the bias.
    ValueStoreFirst,
    /// Build 163 stages the biased value through r0, loads the bias first, and
    /// gives a returned conversion one additional eight-byte frame lane.
    LegacyBiasFirst,
}

/// The version-varying codegen decisions. Every method defaults to the GameCube
/// 2.4.x mainline (mwcceppc build 81 through 2.4.7 build 108); a build that
/// diverges implements this trait and overrides just the differing methods.
pub trait CodegenProfile: core::fmt::Debug {
    /// Whether unused C inline assembly helpers remain as UND symbols: LOCAL
    /// for `static inline`, GLOBAL for plain `inline`. The 2.3.3 line drops
    /// both forms; the 2.4.x line retains them.
    fn retain_unused_c_inline_asm_symbols(&self) -> bool {
        true
    }

    /// Whether unused C++ static-inline assembly helpers remain as LOCAL UND
    /// symbols. The 2.4.2 line retains them; 2.4.7 drops them unless referenced.
    fn retain_unused_cxx_inline_asm_symbols(&self) -> bool {
        false
    }

    /// Whether source-written names on function prototypes consume anonymous
    /// symbol ordinals. Measured on mwcceppc 4.1 build 51213 and Wii build 145;
    /// older generations discard the names without advancing the unit's stream.
    fn prototype_parameter_names_consume_labels(&self) -> bool {
        false
    }

    /// Extra hidden label retained per call-dispatch switch arm by deferred
    /// inlining. This is separate from the ordinary case-body label.
    fn deferred_call_dispatcher_labels_per_case(&self) -> u8 {
        0
    }

    /// Hidden labels retained by the optimizer around a call-dispatch jump
    /// table, independent of labels attributed to individual case arms.
    fn call_dispatcher_hidden_label_bump(&self) -> u8 {
        0
    }

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

    fn int_call_result_conversion_style(&self) -> IntCallResultConversionStyle {
        IntCallResultConversionStyle::ValueStoreFirst
    }

    /// In a non-leaf `if`-prologue, whether the saved-LR store (`stw r0,20(r1)`) is
    /// emitted BEFORE a leading float-constant load in the condition, rather than
    /// filling the `mflr`->store latency slot with that load. GC/2.0p1 and build
    /// 163 store the linkage first; mainline emits `mflr r0; lfs f0,0(0); stw
    /// r0,20; fcmpo`. The same "store before a float load" family as
    /// [`Self::float_cast_value_store_first`].
    fn lr_save_precedes_float_const(&self) -> bool {
        false
    }

    fn folded_float_compare_linkage_style(&self) -> FoldedFloatCompareLinkageStyle {
        FoldedFloatCompareLinkageStyle::LinkRegisterFirst
    }

    /// Anonymous labels retained when a source float guard folds to a
    /// branchless comparison value. The 2.4.x optimizer retains both branch
    /// labels; later profiles override this when their control-flow pass
    /// coalesces one of them.
    fn folded_float_guard_label_bump(&self) -> u8 {
        2
    }

    fn folded_float_guard_ipa_label_bump(&self) -> u8 {
        0
    }

    fn function_ordinal_accounting_style(&self) -> FunctionOrdinalAccountingStyle {
        FunctionOrdinalAccountingStyle::Mainline
    }

    fn long_long_timer_style(&self) -> LongLongTimerStyle {
        LongLongTimerStyle::MainlinePair
    }

    fn nested_global_dispatch_schedule(&self) -> NestedGlobalDispatchSchedule {
        NestedGlobalDispatchSchedule::SequentialAggregateCopy
    }

    fn leading_frame_guard_store_style(&self) -> LeadingFrameGuardStoreStyle {
        LeadingFrameGuardStoreStyle::StoreValueFirstAfterLoad
    }

    fn guarded_member_initialization_style(&self) -> GuardedMemberInitializationStyle {
        GuardedMemberInitializationStyle::IntegerThenPooledFloat
    }

    fn guarded_byte_copy_style(&self) -> GuardedByteCopyStyle {
        GuardedByteCopyStyle::LogicalCompare
    }

    fn frexp_family_style(&self) -> FrexpFamilyStyle {
        FrexpFamilyStyle::VirtualCompactFrame
    }

    /// Anonymous labels retained by the deferred control-flow pass around the
    /// fdlibm `frexp` transaction. Build 81 and the later mainline retain three;
    /// the earlier builds override this with their five-label block.
    fn frexp_deferred_label_bump(&self) -> u8 {
        3
    }

    /// Hidden labels retained by deferred `ldexp` control-flow graphs. Build
    /// 81 and later retain ten.
    fn ldexp_deferred_label_bump(&self) -> u8 {
        10
    }

    fn raise_family_style(&self) -> RaiseFamilyStyle {
        RaiseFamilyStyle::DirectLoadCountRegister
    }

    fn integer_dag_style(&self) -> IntegerDagStyle {
        IntegerDagStyle::WideIssueClosedIntervals
    }

    fn integer_loop_style(&self) -> IntegerLoopStyle {
        IntegerLoopStyle::ModernLatencyInterleaved
    }

    fn mem_copy_word_schedule_style(&self) -> MemCopyWordScheduleStyle {
        MemCopyWordScheduleStyle::PipelinedAlternatingScratch
    }

    fn mem_copy_remainder_mask_style(&self) -> MemCopyRemainderMaskStyle {
        MemCopyRemainderMaskStyle::FusedClearLeft
    }

    fn shared_float_dag_style(&self) -> SharedFloatDagStyle {
        SharedFloatDagStyle::ModernDefinitionDescending
    }

    /// In a float `if`-condition comparing a LOADED value (member/global) against a
    /// pool CONSTANT, whether the value operand is loaded BEFORE the constant.
    /// GC/2.0p1 and build 163 use `lfs f1,(v); lfs f0,k`, while mainline uses
    /// `lfs f0,k; lfs f1,(v)`. The register assignment (`fcmpo f1,f0`) is
    /// unchanged; only the independent load order differs.
    fn float_compare_value_before_const(&self) -> bool {
        false
    }

    /// Whether a first comparison against a short-lived floating local may
    /// materialize its pool literal before the local's memory initializer.
    /// Build 163 schedules the independent literal load into that earlier slot;
    /// later generations keep it adjacent to the comparison.
    fn preload_ephemeral_float_compare_literal(&self) -> bool {
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

    /// Prologue schedule for compiler-generated deleting destructors.
    fn cxx_destructor_prologue_style(&self) -> CxxDestructorPrologueStyle {
        CxxDestructorPrologueStyle::EarlyNullCheck
    }

    /// Saved-LR reload order for a plain linkage-first non-leaf frame.
    fn plain_linkage_epilogue_style(&self) -> PlainLinkageEpilogueStyle {
        PlainLinkageEpilogueStyle::ReloadBeforeStackRestore
    }

    /// Whether a terminal call through a function pointer is lowered as an
    /// unlinked `bctr` sibling call without requiring IPA.
    /// This appears with the 4.x optimizer generation.
    fn terminal_indirect_tail_call(&self) -> bool {
        false
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

    /// Whether an ordered early-return guard with a materialized store tail
    /// emits the store before materializing the continuation's return value.
    fn guard_store_precedes_return_value(&self) -> bool {
        false
    }

    fn narrow_guard_schedule_style(&self) -> NarrowGuardScheduleStyle {
        NarrowGuardScheduleStyle::LatencyInterleavedConsumerTree
    }

    fn va_arg_schedule_style(&self) -> VaArgScheduleStyle {
        VaArgScheduleStyle::LatencyInterleaved
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

    fn computed_store_issue_style(&self) -> ComputedStoreIssueStyle {
        ComputedStoreIssueStyle::ReadinessOrder
    }

    fn value_tracked_mutation_style(&self) -> ValueTrackedMutationStyle {
        ValueTrackedMutationStyle::InlineExpression
    }

    fn negative_power_of_two_multiply_style(&self) -> NegativePowerOfTwoMultiplyStyle {
        NegativePowerOfTwoMultiplyStyle::ShiftThroughScratch
    }

    fn field_merge_style(&self) -> FieldMergeStyle {
        FieldMergeStyle::RightBaseElideCoveredMask
    }

    fn return_register_store_style(&self) -> ReturnRegisterStoreStyle {
        ReturnRegisterStoreStyle::SourceOrder
    }

    fn comma_value_placement_style(&self) -> CommaValuePlacementStyle {
        CommaValuePlacementStyle::RegisterResident
    }

    fn global_array_index_style(&self) -> GlobalArrayIndexStyle {
        GlobalArrayIndexStyle::Indexed
    }

    fn global_array_decay_store_style(&self) -> GlobalArrayDecayStoreStyle {
        GlobalArrayDecayStoreStyle::ScratchValue
    }

    fn indexed_rmw_assignment_style(&self) -> IndexedRmwAssignmentStyle {
        IndexedRmwAssignmentStyle::UniformIndexed
    }

    fn stored_global_read_style(&self) -> StoredGlobalReadStyle {
        StoredGlobalReadStyle::ReuseStoredRegister
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

    /// Hidden labels retained by the deferred k_cos-style else composition.
    /// Build 81 and later retain two; earlier builds override with seven.
    fn punned_float_composition_deferred_label_bump(&self) -> u8 {
        2
    }

    fn punned_conditional_writeback_style(&self) -> PunnedConditionalWritebackStyle {
        PunnedConditionalWritebackStyle::Optimized
    }

    fn punned_shift_writeback_style(&self) -> PunnedShiftWritebackStyle {
        PunnedShiftWritebackStyle::LiveParameter
    }

    fn trig_dispatcher_style(&self) -> TrigDispatcherStyle {
        TrigDispatcherStyle::LiveParameter
    }

    fn trig_zero_constant_placement(&self) -> TrigZeroConstantPlacement {
        TrigZeroConstantPlacement::SmallArm
    }

    /// Hidden labels added to every trigonometric dispatcher's pool counter.
    fn trig_dispatcher_hidden_label_bump(&self) -> u8 {
        0
    }

    /// Additional hidden labels retained when whole-file IPA is enabled.
    fn trig_dispatcher_ipa_label_bump(&self) -> u8 {
        0
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

    fn data_section_relocation_style(&self) -> DataSectionRelocationStyle {
        DataSectionRelocationStyle::NamedObject
    }

    fn data_section_anchor_comment_flags(&self) -> u32 {
        self.read_only_section_anchor_comment_flags()
    }

    /// Whether unused function prototypes carrying a section attribute remain
    /// visible as GLOBAL UND symbols in the emitted object.
    fn materialize_section_prototypes(&self) -> bool {
        false
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

    /// Anonymous-symbol weights for structural facts from in-class C++ inline
    /// definitions. The 2.4.x generation only exposes the virtual-destructor
    /// artifact; GC 4.1 overrides the optimizer-analysis costs.
    fn cxx_class_definition_label_bump(&self) -> u8 {
        0
    }

    fn cxx_inline_definition_label_bump(&self) -> u8 {
        0
    }

    fn cxx_inline_control_flow_label_weight(&self) -> u8 {
        1
    }

    fn cxx_virtual_destructor_label_bump(&self) -> u8 {
        2
    }

    fn cxx_inline_ipa_call_label_bump(&self) -> u8 {
        0
    }

    /// Anonymous-ordinal cost of RTTI's class-declaration analysis. The first
    /// polymorphic declaration shares one baseline block; later declarations
    /// advance by their kind-specific weight.
    fn cxx_rtti_virtual_method_label_weight(&self, _whole_file: bool) -> u8 {
        4
    }

    fn cxx_rtti_virtual_destructor_label_weight(&self, _whole_file: bool) -> u8 {
        6
    }

    fn cxx_rtti_inherited_virtual_destructor_label_bump(&self, _whole_file: bool) -> u8 {
        2
    }

    fn cxx_rtti_initial_virtual_label_discount(&self, _whole_file: bool) -> u8 {
        4
    }

    fn cxx_rtti_inline_definition_label_bump(&self) -> u8 {
        self.cxx_inline_definition_label_bump()
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

    fn fixed_address_parameterized_rmw_style(&self) -> FixedAddressParameterizedRmwStyle {
        FixedAddressParameterizedRmwStyle::Mainline24
    }

    fn fixed_address_constant_store_style(&self) -> FixedAddressConstantStoreStyle {
        FixedAddressConstantStoreStyle::ValueFirst
    }

    fn fixed_address_poll_address_style(&self) -> FixedAddressPollAddressStyle {
        FixedAddressPollAddressStyle::MaterializedElementForNonzeroIndex
    }

    fn queue_service_inlining_style(&self) -> QueueServiceInliningStyle {
        QueueServiceInliningStyle::InlineVerifiedCallers
    }

    fn narrow_compound_shift_style(&self) -> NarrowCompoundShiftStyle {
        NarrowCompoundShiftStyle::ImmediateInScratch
    }

    fn logical_or_value_style(&self) -> LogicalOrValueStyle {
        LogicalOrValueStyle::FalseFirst
    }
}

/// GameCube 2.4.7 mainline — the reference behavior (all defaults). Covers
/// GC/2.0, 2.5, 2.6, and 2.7.
#[derive(Debug)]
pub struct Mainline;
impl CodegenProfile for Mainline {}

/// GC/2.5 through GC/2.7 share an aggregate-copy issue order that differs from build 92 while
/// retaining every other mainline policy.
#[derive(Debug)]
pub struct MainlineEarlyAggregateLoads;
impl CodegenProfile for MainlineEarlyAggregateLoads {
    fn nested_global_dispatch_schedule(&self) -> NestedGlobalDispatchSchedule {
        NestedGlobalDispatchSchedule::EarlyAggregateLoads
    }
}

/// GC/3.0a3 — mwcceppc 4.1 build 51213. Differential characterization shows
/// a substantial optimizer transition from the 2.4.7 generation. This profile
/// deliberately has its own identity even while uncharacterized behaviors fall
/// back to the shared defaults, so each measured 4.1 behavior can be added here
/// without changing the older mainline builds.
#[derive(Debug)]
pub struct Gc41Build51213;
impl CodegenProfile for Gc41Build51213 {
    fn cxx_destructor_prologue_style(&self) -> CxxDestructorPrologueStyle {
        CxxDestructorPrologueStyle::SavedHomesBeforeNullCheck
    }

    fn guarded_byte_copy_style(&self) -> GuardedByteCopyStyle {
        GuardedByteCopyStyle::SignedCompare
    }

    fn guarded_member_initialization_style(&self) -> GuardedMemberInitializationStyle {
        GuardedMemberInitializationStyle::PooledFloatThenInteger
    }

    fn long_long_timer_style(&self) -> LongLongTimerStyle {
        LongLongTimerStyle::Unmodeled
    }

    fn prototype_parameter_names_consume_labels(&self) -> bool {
        true
    }

    fn deferred_call_dispatcher_labels_per_case(&self) -> u8 {
        1
    }

    fn call_dispatcher_hidden_label_bump(&self) -> u8 {
        3
    }

    fn folded_float_guard_label_bump(&self) -> u8 {
        3
    }

    fn folded_float_guard_ipa_label_bump(&self) -> u8 {
        1
    }

    fn function_ordinal_accounting_style(&self) -> FunctionOrdinalAccountingStyle {
        FunctionOrdinalAccountingStyle::Gc41
    }

    fn cxx_class_definition_label_bump(&self) -> u8 {
        1
    }

    fn cxx_inline_definition_label_bump(&self) -> u8 {
        4
    }

    fn cxx_inline_control_flow_label_weight(&self) -> u8 {
        0
    }

    fn cxx_virtual_destructor_label_bump(&self) -> u8 {
        3
    }

    fn cxx_inline_ipa_call_label_bump(&self) -> u8 {
        1
    }

    fn cxx_rtti_virtual_method_label_weight(&self, whole_file: bool) -> u8 {
        if whole_file {
            4
        } else {
            5
        }
    }

    fn cxx_rtti_virtual_destructor_label_weight(&self, whole_file: bool) -> u8 {
        if whole_file {
            7
        } else {
            9
        }
    }

    fn cxx_rtti_inherited_virtual_destructor_label_bump(&self, whole_file: bool) -> u8 {
        if whole_file {
            0
        } else {
            4
        }
    }

    fn fixed_address_parameterized_rmw_style(&self) -> FixedAddressParameterizedRmwStyle {
        FixedAddressParameterizedRmwStyle::Modern4x
    }

    fn terminal_indirect_tail_call(&self) -> bool {
        true
    }

    fn global_array_decay_store_style(&self) -> GlobalArrayDecayStoreStyle {
        GlobalArrayDecayStoreStyle::DirectAddress
    }

    fn trig_zero_constant_placement(&self) -> TrigZeroConstantPlacement {
        TrigZeroConstantPlacement::Prologue
    }

    fn trig_dispatcher_hidden_label_bump(&self) -> u8 {
        3
    }

    fn trig_dispatcher_ipa_label_bump(&self) -> u8 {
        4
    }
}

/// Wii/1.0 — mwcceppc 4.3 build 145. Kept separate from the 4.1 GameCube
/// profile so measured optimizer transitions remain scoped to this generation.
#[derive(Debug)]
pub struct Wii43Build145;
impl CodegenProfile for Wii43Build145 {
    fn guarded_byte_copy_style(&self) -> GuardedByteCopyStyle {
        GuardedByteCopyStyle::SignedCompareWithAlignedStore
    }

    fn guarded_member_initialization_style(&self) -> GuardedMemberInitializationStyle {
        GuardedMemberInitializationStyle::PooledFloatThenInteger
    }

    fn cxx_rtti_virtual_method_label_weight(&self, whole_file: bool) -> u8 {
        if whole_file {
            4
        } else {
            5
        }
    }

    fn cxx_rtti_virtual_destructor_label_weight(&self, whole_file: bool) -> u8 {
        if whole_file {
            7
        } else {
            9
        }
    }

    fn cxx_rtti_inherited_virtual_destructor_label_bump(&self, whole_file: bool) -> u8 {
        if whole_file {
            0
        } else {
            4
        }
    }

    fn cxx_rtti_inline_definition_label_bump(&self) -> u8 {
        4
    }

    fn long_long_timer_style(&self) -> LongLongTimerStyle {
        LongLongTimerStyle::Unmodeled
    }

    fn cxx_inline_control_flow_label_weight(&self) -> u8 {
        0
    }

    fn prototype_parameter_names_consume_labels(&self) -> bool {
        true
    }

    fn deferred_call_dispatcher_labels_per_case(&self) -> u8 {
        1
    }

    fn fixed_address_parameterized_rmw_style(&self) -> FixedAddressParameterizedRmwStyle {
        FixedAddressParameterizedRmwStyle::Modern4x
    }

    fn fixed_address_constant_store_style(&self) -> FixedAddressConstantStoreStyle {
        FixedAddressConstantStoreStyle::BaseFirst
    }

    fn terminal_indirect_tail_call(&self) -> bool {
        true
    }

    fn global_array_decay_store_style(&self) -> GlobalArrayDecayStoreStyle {
        GlobalArrayDecayStoreStyle::DirectAddress
    }

    fn trig_zero_constant_placement(&self) -> TrigZeroConstantPlacement {
        TrigZeroConstantPlacement::Prologue
    }

    fn trig_dispatcher_hidden_label_bump(&self) -> u8 {
        3
    }

    fn trig_dispatcher_ipa_label_bump(&self) -> u8 {
        4
    }
}

/// GC/1.3 — mwcceppc 2.4.2 build 53. The early 2.4.2 build that defaulted plain
/// `char` to unsigned, before build 81 restored signed.
#[derive(Debug)]
pub struct Gc13Build53;
impl CodegenProfile for Gc13Build53 {
    fn retain_unused_cxx_inline_asm_symbols(&self) -> bool {
        true
    }

    fn fixed_address_parameterized_rmw_style(&self) -> FixedAddressParameterizedRmwStyle {
        FixedAddressParameterizedRmwStyle::Early24
    }

    fn char_is_signed(&self) -> bool {
        false
    }

    fn data_section_relocation_style(&self) -> DataSectionRelocationStyle {
        DataSectionRelocationStyle::SectionAnchor
    }

    fn fixed_address_poll_address_style(&self) -> FixedAddressPollAddressStyle {
        FixedAddressPollAddressStyle::FoldedBankDisplacement
    }

    fn queue_service_inlining_style(&self) -> QueueServiceInliningStyle {
        QueueServiceInliningStyle::KeepServiceCallOutOfLine
    }

    fn mem_copy_word_schedule_style(&self) -> MemCopyWordScheduleStyle {
        MemCopyWordScheduleStyle::SerialScratch
    }

    fn mem_copy_remainder_mask_style(&self) -> MemCopyRemainderMaskStyle {
        MemCopyRemainderMaskStyle::MaterializedThree
    }

    fn trig_dispatcher_style(&self) -> TrigDispatcherStyle {
        TrigDispatcherStyle::EarlyLiveParameter
    }

    fn frexp_deferred_label_bump(&self) -> u8 {
        5
    }

    fn ldexp_deferred_label_bump(&self) -> u8 {
        20
    }

    fn punned_float_composition_deferred_label_bump(&self) -> u8 {
        7
    }
}

/// GC/1.3.2 — mwcceppc 2.4.2 build 81. Its MSL aligned-copy loop still uses
/// the serial single-scratch schedule; the pipelined two-register form arrives
/// with the 2.4.7 line.
#[derive(Debug)]
pub struct Gc132Build81;
impl CodegenProfile for Gc132Build81 {
    fn retain_unused_cxx_inline_asm_symbols(&self) -> bool {
        true
    }

    fn mem_copy_word_schedule_style(&self) -> MemCopyWordScheduleStyle {
        MemCopyWordScheduleStyle::SerialScratch
    }
}

/// GC/1.2.5[n] — mwcceppc 2.3.3 build 163. Its first measured architectural
/// difference is the linkage-first stack frame; additional scheduler differences
/// remain under characterization, so this profile is experimental.
#[derive(Debug)]
pub struct Gc233Build163 {
    plain_linkage_epilogue_style: PlainLinkageEpilogueStyle,
}

pub const GC233_BUILD163: Gc233Build163 = Gc233Build163 {
    plain_linkage_epilogue_style: PlainLinkageEpilogueStyle::ReloadBeforeStackRestore,
};

pub const GC233_BUILD159_PATCH1: Gc233Build163 = Gc233Build163 {
    plain_linkage_epilogue_style: PlainLinkageEpilogueStyle::StackRestoreBeforeReload,
};

impl CodegenProfile for Gc233Build163 {
    fn guarded_member_initialization_style(&self) -> GuardedMemberInitializationStyle {
        GuardedMemberInitializationStyle::LazyPooledFloat
    }

    fn cxx_rtti_virtual_method_label_weight(&self, _whole_file: bool) -> u8 {
        1
    }

    fn cxx_rtti_virtual_destructor_label_weight(&self, _whole_file: bool) -> u8 {
        3
    }

    fn cxx_rtti_initial_virtual_label_discount(&self, _whole_file: bool) -> u8 {
        1
    }

    fn retain_unused_c_inline_asm_symbols(&self) -> bool {
        false
    }

    fn long_long_timer_style(&self) -> LongLongTimerStyle {
        LongLongTimerStyle::Unmodeled
    }

    fn cxx_inline_control_flow_label_weight(&self) -> u8 {
        0
    }

    fn frame_convention(&self) -> FrameConvention {
        FrameConvention::LinkageFirst
    }

    fn plain_linkage_epilogue_style(&self) -> PlainLinkageEpilogueStyle {
        self.plain_linkage_epilogue_style
    }

    fn data_section_relocation_style(&self) -> DataSectionRelocationStyle {
        DataSectionRelocationStyle::SectionAnchor
    }

    fn materialize_section_prototypes(&self) -> bool {
        true
    }

    fn emit_leaf_frame_unwind(&self) -> bool {
        false
    }

    fn constant_join_return_precedes_lr_reload(&self) -> bool {
        true
    }

    fn guard_store_precedes_return_value(&self) -> bool {
        true
    }

    fn narrow_guard_schedule_style(&self) -> NarrowGuardScheduleStyle {
        NarrowGuardScheduleStyle::CompareFirstDeclarationOrder
    }

    fn va_arg_schedule_style(&self) -> VaArgScheduleStyle {
        VaArgScheduleStyle::SerialScratch
    }

    fn legacy_float_cast_schedule(&self) -> bool {
        true
    }

    fn int_call_result_conversion_style(&self) -> IntCallResultConversionStyle {
        IntCallResultConversionStyle::LegacyBiasFirst
    }

    fn lr_save_precedes_float_const(&self) -> bool {
        true
    }

    fn folded_float_compare_linkage_style(&self) -> FoldedFloatCompareLinkageStyle {
        FoldedFloatCompareLinkageStyle::CompareFirst
    }

    fn leading_frame_guard_store_style(&self) -> LeadingFrameGuardStoreStyle {
        LeadingFrameGuardStoreStyle::GuardHighFirstAfterDataUse
    }

    fn frexp_family_style(&self) -> FrexpFamilyStyle {
        FrexpFamilyStyle::LegacyPhysicalFrame
    }

    fn frexp_deferred_label_bump(&self) -> u8 {
        5
    }

    fn ldexp_deferred_label_bump(&self) -> u8 {
        20
    }

    fn raise_family_style(&self) -> RaiseFamilyStyle {
        RaiseFamilyStyle::StagedLoadLinkRegister
    }

    fn integer_dag_style(&self) -> IntegerDagStyle {
        IntegerDagStyle::PortAwareSerialR0
    }

    fn integer_loop_style(&self) -> IntegerLoopStyle {
        IntegerLoopStyle::LegacyDependencyFirst
    }

    fn mem_copy_word_schedule_style(&self) -> MemCopyWordScheduleStyle {
        MemCopyWordScheduleStyle::SerialScratch
    }

    fn shared_float_dag_style(&self) -> SharedFloatDagStyle {
        SharedFloatDagStyle::LegacyBalancedPrefix
    }

    fn float_compare_value_before_const(&self) -> bool {
        true
    }

    fn preload_ephemeral_float_compare_literal(&self) -> bool {
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
    fn computed_store_issue_style(&self) -> ComputedStoreIssueStyle {
        ComputedStoreIssueStyle::EvaluationOrder
    }
    fn value_tracked_mutation_style(&self) -> ValueTrackedMutationStyle {
        ValueTrackedMutationStyle::InPlaceResultRegister
    }
    fn negative_power_of_two_multiply_style(&self) -> NegativePowerOfTwoMultiplyStyle {
        NegativePowerOfTwoMultiplyStyle::ShiftInResultRegister
    }
    fn field_merge_style(&self) -> FieldMergeStyle {
        FieldMergeStyle::LeftBasePreserveMask
    }
    fn return_register_store_style(&self) -> ReturnRegisterStoreStyle {
        ReturnRegisterStoreStyle::DelayLeadingResultStoreOneSlot
    }
    fn comma_value_placement_style(&self) -> CommaValuePlacementStyle {
        CommaValuePlacementStyle::ParameterHome
    }

    fn global_array_index_style(&self) -> GlobalArrayIndexStyle {
        GlobalArrayIndexStyle::ExplicitAddress
    }
    fn indexed_rmw_assignment_style(&self) -> IndexedRmwAssignmentStyle {
        IndexedRmwAssignmentStyle::PreserveExplicitAddress
    }
    fn stored_global_read_style(&self) -> StoredGlobalReadStyle {
        StoredGlobalReadStyle::ReloadAfterStore
    }
    fn negate_before_zero_equality(&self) -> bool {
        true
    }
    fn punned_float_frame_convention(&self) -> PunnedFloatFrameConvention {
        PunnedFloatFrameConvention::LegacyReloading
    }
    fn punned_float_composition_deferred_label_bump(&self) -> u8 {
        7
    }
    fn punned_conditional_writeback_style(&self) -> PunnedConditionalWritebackStyle {
        PunnedConditionalWritebackStyle::BranchDiamond
    }
    fn punned_shift_writeback_style(&self) -> PunnedShiftWritebackStyle {
        PunnedShiftWritebackStyle::LegacyReloading
    }
    fn trig_dispatcher_style(&self) -> TrigDispatcherStyle {
        TrigDispatcherStyle::LegacyReloading
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

    fn fixed_address_parameterized_rmw_style(&self) -> FixedAddressParameterizedRmwStyle {
        FixedAddressParameterizedRmwStyle::Legacy233
    }

    fn fixed_address_constant_store_style(&self) -> FixedAddressConstantStoreStyle {
        FixedAddressConstantStoreStyle::BaseFirst
    }

    fn fixed_address_poll_address_style(&self) -> FixedAddressPollAddressStyle {
        FixedAddressPollAddressStyle::MaterializedBankPage
    }

    fn queue_service_inlining_style(&self) -> QueueServiceInliningStyle {
        QueueServiceInliningStyle::KeepServiceCallOutOfLine
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
    fn guarded_member_initialization_style(&self) -> GuardedMemberInitializationStyle {
        GuardedMemberInitializationStyle::ReloadFloatPerStore
    }

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
