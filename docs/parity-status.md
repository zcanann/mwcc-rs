# Reference-project parity status

Last fresh holdout: 2026-07-23 22:53 UTC at compiler commit `c0962f28`

Latest paired checkpoint: 2026-07-23 17:44 UTC at compiler commit `869596ad`

Latest targeted checkpoint: 2026-09-08, load-field insertion and implicit return liveness (fingerprint below)

Latest measured compiler + harness fingerprint: `9bdb4a997d9e9460be021c15d53dc8110136c8ab820c5be06a77b74585397833:5e4ca1ddc460f4d86cd15e9e7a834f5b2a572a7e0c80e09279a629da4eac0806`

This file records a measurement checkpoint, not a claim that the numbers stay
current after compiler or harness changes. Canary and work-queue counts are
labeled diagnostics; neither is a corpus parity estimate.

## Load-field insertion and implicit return liveness, 2026-09-08

A separate expression owner proves which bits unsigned loads, integer casts,
constant shifts, masks, and OR trees can set. Disjoint fields use `rlwimi`
instead of separate shift/OR instructions at O2–O4. Load reordering requires
frontend evidence of a nonvolatile pointer; volatile and unknown addresses keep
their existing owner. Overlapping fields and widening casts of signed loads do
not acquire an invalid zero-extension proof. Register placement remains separate
from mask analysis, leaving version-specific scheduling work independently
replaceable.

Canaries **2273–2275** add **27 functions**, measured across fifteen releases and
six modes: O4, O3, O2, schedule off, optimize for size, and O0. Complete sample
compilation improves **180 → 255/270**; exact function matches improve
**60 → 396/2,430**, with **336 gains and no exact losses**. The remaining fifteen
compilation failures are the masked-load sample at O0, where the existing
compound-load guard still defers. Native execution passes **153,600 candidate
function scenarios** and **155,520 reference scenarios**, including truncating
shifts, signed inputs, narrow casts, overlapping fields, aliasing stores, retained
pointers, and memory/ABI guards. Volatile controls preserve the baseline's access
traces; this does not claim their traces match every reference version.

A retained-pointer control exposed missing implicit ABI return uses in liveness.
Allocation now receives result registers from the function signature: integer
`r3`, integer-pair `r3:r4`, or floating `f1`. Ordinary and conditional returns
consume those values through the same CFG analysis as explicit operands. Void
functions gain no artificial result occupancy, and subsequent definitions still
allow reuse. Tests cover both classes, pair-result homes, conditional returns,
and reuse after the incoming value dies. All **5,760** baseline retained-pointer
failures in the new panel are fixed, including O0.

The same fix corrects returned values in the real Dolphin DVD queue pop/check
functions. **256 queue scenarios** pass against the reference and an independent
priority/FIFO model on GC/1.2.5n and GC/1.3; the baseline fails all 256. Queues are
constructed independently to isolate those changed functions. A broader probe
found a separate, unchanged defect in `__DVDPushWaitingQueue`: an address
temporary overwrites the implicit `OSRestoreInterrupts` argument. That remains a
next target; this checkpoint does not claim full DVD queue correctness.

The preceding MD5/canary panel retains **6,720 passing candidate scenarios**,
including all **960 MD5 digest cases**. Full MD5 still compiles on **15/15**
versions; its fourteen accepted reference units remain **0/84** byte-exact.
Field insertion reduces each measured MD5 unit by twelve text bytes, but loop
unrolling, scheduling, and placement remain substantial matching work. The Wii
reference still rejects the project's duplicate `s8` declaration.

Of **2,865** existing canary combinations, **2,575** successful objects and **276**
failure diagnostics are unchanged. The remaining fourteen objects are endian
stack unpacking, with no exact losses. Its 16/32-bit helpers pass **1,920 native
scenarios**, fixing **448** baseline return-value failures. The source's 64-bit
helper has an undersized scratch array and is excluded from native assertions;
all three helpers are included in byte comparisons. The 302-unit Dolphin panel
retains **191** compiled units; only MD5 and DVD queue objects change, on both
versions. All **187** other successful objects remain identical.

Validation passes **2,166 tests** (351 app, 1,697 backend, 118 allocator), with
the previously established nine app failures and one nested-assembly backend
failure excluded; eight allocator search tests remain ignored. No full corpus or
holdout claim is made. `target/load-field-final-verification.json` binds the
compiler/harness fingerprints, sources, comparison objects, and **995 native
object entries**. Measurements and execution records are under
`target/load-field-{canaries,parity,regressions,frontier,unpack}/` and
`target/load-field-queue-execution.json`.

## Wide virtual IDs and full Dolphin MD5, 2026-09-08

Compiler fingerprint: `f551c24bb192f0cc729229028dd49ccdda1e7496fd9f6265debbd841aa199e3e`.

Selected GPR/FPR fields now retain 32-bit virtual IDs through instruction
selection, frame planning, liveness, and scheduling. Physical allocator homes
remain byte-sized numbers in 0–31. The encoder rejects unresolved register
fields, and checked field conversion prevents IDs from wrapping into physical
registers. This removes the former 224-temporary limit per register class.
Unit tests cover 1,024 sequential virtuals, both register classes, field-capacity
boundaries, and unresolved operands at encoding.

The complete Battle for Bikini Bottom Dolphin `eth/md5.c` now compiles on
**15/15** versions, up from **0/15**. Progress past `MD5Transform` exposed two
more issues: a guard rejected the shift/OR load tree in `Decode`, and indexed
loads/stores with constant offsets modified their original pointer bases.
Load-packing trees now use ordinary virtual operand placement. Indexed addresses
receive separate virtual homes, preserving pointers across subsequent accesses,
loop iterations, and implicit ABI returns.

Native execution verifies **960 MD5 digest scenarios** across all fifteen
candidate versions against Python's independent digest implementation, including
empty messages, padding/block boundaries, and fragmented updates. The fourteen
GC references pass the same cases. The Wii/1.0 reference rejects this project's
headers for a redeclared `s8`; its candidate is checked against the independent
model only. The RSA Data Security, Inc. MD5 Message-Digest Algorithm source in
the reference project is used unchanged. None of the **84** functions emitted
by the fourteen accepted reference units is byte-exact yet.

Canaries **2270–2272** cover byte packing, 64 unrolled arithmetic rounds, and
retained bases in indexed stores. Across O4/O0 and fifteen builds, complete
sample compilation improves **30 → 90/90**; exact function matches improve
**30 → 54/210**. The native panel passes **6,720 scenarios**, including the 960
full MD5 cases. The former compiler fails **945** retained-store scenarios;
all corresponding candidate and reference executions pass. This panel checks
results, affected memory, saved GPRs, stack restoration, and return completion.

The fresh 151-source Dolphin panel improves **189 → 191/302** compiled units;
reference-supported compilation improves **187 → 189/293**. Both gains are
`eth/md5.c` (GC/1.2.5n and GC/1.3). All **189** previously compiling project
objects remain identical. The focused existing canary panel retains **2,589**
identical objects and **276** identical failure diagnostics (2,865 combinations).

Validation passes **2,278 unit/integration tests**: 348 app, 1,697 backend, 116
allocator, 74 debug-info, 33 object-writer, and 10 encoding tests. Nine app tests
were independently confirmed failing at baseline `b63945f6` and excluded from
the final pass, alongside the known nested-inline-assembly backend test. Eight
allocator search tests retain their existing ignored status. No full corpus or
holdout claim is made by this checkpoint.

The compiler fingerprint for this checkpoint binds its objects. The local manifest
`target/vreg-wide-final-verification.json` verifies all comparison hashes and
**239 native object bindings**, with source, native-harness, and original helper
DOL hashes. Measurements are in `target/vreg-wide-parity/`,
`target/vreg-wide-regressions/`, and `target/vreg-wide-frontier/`.

## Broader Dolphin probe and shifted expressions, 2026-09-08

A fresh probe covers **151 Dolphin C sources × two compiler versions**
(GC/1.2.5n and GC/1.3), using the existing Battle for Bikini Bottom include and
compiler configuration. The references accept **293/302** combinations. The
candidate initially compiled **187/302**, including two GXStubs combinations
that the reference rejects for a missing header. Among reference-supported
combinations, compilation improves **185 → 187/293**; total candidate compilation
improves **187 → 189/302**. Both gains are the complete `dsp/dsp.c` unit. All
**187** previously compiling project objects remain byte-identical.

The complete DSP source now compiles on **15/15** versions, up from **0/15**.
Its mailbox reader passes **3,840** native cases against reference and original
GQPE78 game models, checking both halfword reads in order, all input memory,
saved registers, stack, and return state. It remains **0/15** byte-exact: the
reference shares the register-bank base, retains halfword masks, and merges the
fields with `rlwimi`. Compilation progress does not imply full DSP matching.

A recursive pre-selection rejection had hidden existing field-merge and virtual
operand-placement implementations whenever a constant shift appeared on the
left of a commutative expression. Removing it lets those owners lower constant
rotates, shifted arithmetic and logic, and mailbox assembly. Two shifted load
operands also use the existing virtual homes, keeping the left result live while
the right uses scratch. Other compound-load forms retain their existing guard.
Address-of lowering now resolves constant elements of absolute register-bank
arrays without treating the bank as an unknown scalar or reading its contents;
this also supports word stores through a cast halfword-array address.

Canaries **2264–2269** exercise eighteen functions in six modes across fifteen
builds. Complete sample units improve **0 → 90/90**. Individual baseline probes
compile **90/1,620** functions; all **1,620** candidate and reference functions
compile. Exact function matches improve **0 → 761/1,620** with no exact losses.
The samples cover constant rotates, arithmetic-right-shift controls, shifted
arithmetic and bitwise combinations, halfword packing, shared-pointer loads,
volatile and fixed-address mailbox reads, masking, separate field sources,
absolute element addresses, and punned register-bank writes. All **103,680**
native cases pass reference results, volatile access traces, memory guards,
saved registers, stack, and return-state checks.

Two preceding sample panels cover **2,775 objects**. All **2,499** successful
objects remain byte-identical, and all **276** failures preserve their diagnostics.
Backend tests pass **1,697** with the existing nested-asm exclusion. The checkpoint
has **107,520** distinct native cases; final recompilation reproduces all **210**
execution-tested object entries. `target/shift-combine-final-verification.json`
binds the compiler and harness, sample and project sources, compilation objects,
native harnesses and objects, and original DOL fingerprint.

The broader probe leaves **106** reference-supported compilation failures.
OS audio initialization advances past shifted reads and the absolute bank address
to a call-bearing tick comparison. MD5 advances to a panic at the current
224-virtual-register ID ceiling; it remains uncompiled. Other measured work
includes global/member address handling, callback arguments, inline expansion,
and long-long operations. The nine reference failures are configuration or
source limitations in this probe, including missing declarations/headers and a
GC/1.3 assembly-frame rejection. These are targeted measurements, not a full
project-build or whole-corpus parity claim.

## C and C++ narrow comparison returns, 2026-09-08

The real `GXGetTexObjMipMap` now matches all bytes on GC/1.1, 1.1p1, 1.2.5,
and 1.2.5n, improving complete matches **3 → 7/15**. All **3,840** exhaustive
flag-byte execution cases pass against baseline, reference, and original GQPE78
models. The AX/GX two-version library panel remains **48/48** compiling and
changes only GC/1.2.5n's texture unit. The three-unit, fifteen-version project
panel remains **45/45** compiling; the four early versions change only the
mipmap getter and `GXGetTexObjEdgeLOD`. The latter also gains the final byte
conversion but retains a different bit-extraction sequence. Its **3,840** native
cases pass reference and original game models.
All fifteen complete AXVPB objects remain identical.

Early MWCC distinguishes the source languages here: C comparisons produce an
integer truth value followed by the declared byte/halfword conversion, while
C++ comparisons keep their existing boolean result path. Lowering now retains
the existing source-language fact independently of symbol linkage and uses it
inside the legacy full-width return policy. C++ `extern "C"` functions therefore
retain C++ behavior. Ordinary narrow arithmetic and explicit casts keep their
existing paths. The unsigned return owner also reuses a terminal one-bit carry
chain mask in the result register, avoiding an extra narrow mask.

Canaries **2252–2263** pair eighteen C/C++ functions in six modes across fifteen
builds. All **180 objects / 3,240 functions** compile on baseline, candidate, and
reference. Exact function matches improve **1,758 → 1,882/3,240**, with no exact
losses. All C++ candidate objects retain the same function bytes and relocations.
The controls cover signed and unsigned byte/halfword returns, comparisons and
logical not, explicit casts, arithmetic returns, word returns, source `bool`, and
unmangled C++ linkage. All **103,680** native cases pass, respecting the declared
low-bit ABI for ordinary narrow arithmetic returns.

The preceding boolean samples **2246–2251** improve **214 → 314/1,080** exact
functions, with no exact losses. All **34,560** native cases pass against the
references, including promoted operands, shared pointers, callback order and
clobbers, memory guards, saved registers, and return state. The two sample panels
gain **224** exact functions in total. An additional **1,605** earlier objects
preserve all **1,260** successful objects byte-for-byte and all **345** failure
diagnostics. The checkpoint has **145,920** distinct native cases.

Backend tests pass **1,697** with the existing nested-asm exclusion. Two new
unit tests exercise language provenance independently of linkage and reuse of
the carry-chain result mask. Final recompilation reproduces all **810** native
object entries. `target/boolean-tail-final-verification.json` binds the compiler,
harness, sources, compilation objects, native harnesses and objects, and original
DOL fingerprint. These targeted results do not establish whole-project parity;
other versions of the getter and scheduling in the newly compiling AX/GX units
remain follow-up work.

## Narrow booleans and AX/GX compilation, 2026-09-08

The three GC/1.3 failures from the preceding checkpoint now compile: `AXOut`,
`GXInit`, and `GXTexture`. The 24-unit AX/GX panel across GC/1.2.5n and GC/1.3
improves **45 → 48/48**. Across all fifteen compiler versions, those three
translation units improve **12 → 45/45**, with all references also compiling.
The only changed function in previously compiling project objects is
`GXGetTexObjMipMap`; its exact matches improve **0 → 3/15** on GC/1.3, 1.3.2,
and 1.3.2r. `__AXOutInitDSP` compiles and executes correctly but remains
**0/15** exact. This is compilation and targeted code-generation progress,
not complete project matching.

A shared result-range predicate identifies canonical integer booleans without
skipping promotion of their operands. Narrow returns retain signed extension
and fold unsigned truncation into the final logical shift where possible.
Computed comparisons hold one operand in a virtual register; two call-bearing
operands follow the reference's right-before-left order. Short-circuit values
can use an independent virtual accumulator, preserving a shared pointer needed
by the second test. The existing dependency-ordered argument planner now accepts
pure conditionals and proven narrow boolean or same-type member arguments.
Allocation can add the first saved register to an existing canonical call frame,
fixing DSP initialization and values held across comparison calls. Masked loads
use the existing version-specific computed-equality policy.

Canaries **2246–2251** cover twelve functions in six modes across fifteen builds:
byte and halfword returns, signed extension, promoted arithmetic, wrapping word
arithmetic, shared-pointer short circuits, narrow call arguments, and two calls
inside equality. Complete sample translation units improve **0 → 90/90**;
individual-function baseline probes compile **96/1,080**, and the candidate
compiles all **1,080**. Whole-function exact matches improve **0 → 214/1,080**.
All **34,560** native sample cases pass against the references, including
callback order, volatile register clobbers, memory guards, argument values,
saved registers, stack, and return state.

The preceding 68-sample panel has **1,020** objects: **942** successful objects
remain identical, **71** failures retain identical diagnostics, and seven
successful objects change only `volatile_bit`. Those seven functions remain
nonmatching but pass **3,584** execution cases, including exact volatile access
traces. There are no compilation or exact-match losses in the measured panels.
The complete real DSP initializer passes **240** cases, and the real texture
mipmap getter passes **3,840** cases covering every byte flag. Both run against
reference and original GQPE78 game models, with baseline comparisons where
compilation was already supported. Total distinct native cases: **42,224**.

Backend tests pass **1,695**, with the existing nested-asm exclusion. Six new
unit tests cover return coercion, call order and frame ownership, shared-pointer
lifetimes, narrow arguments, and boolean range rejection. Final recompilation
reproduces all **262** execution-tested object entries. All fifteen full AXVPB
objects remain identical to the preceding checkpoint, preserving its initializer
matches. GC/1.2.5n's library panel changes only the mipmap getter; GC/1.3's only
changes are the three newly compiling units.

`target/narrow-bool-final-verification.json` binds the final compiler and harness,
canary sources, compilation objects, execution harnesses, native object entries,
and original DOL fingerprint. These are targeted results, not a full-corpus
parity estimate. Remaining work includes scheduling and register choices in the
newly compiling functions and the successive clear loops of later AXVPB builds.

## Later quotient and first-fill scheduling, 2026-09-08

The real `__AXVPBInit` now matches through its first clear loop, **116 bytes and
relocations**, on GC/1.3, 1.3.2, 1.3.2r, 2.0, 2.0p1, 2.5, 2.6, and 2.7. Together
with the four already-complete early versions, this prefix improves **4 → 12/15**.
All eleven later-version initializers change only within those first 116 bytes;
the newest three retain different frames and anchor registers. Complete matches
remain **4/15**, and `__AXSetPBDefault` remains **15/15** exact. All **240** real
initializer execution cases pass baseline, candidate, reference, and original
GQPE78 game models.

A backend nomination preserves the identities of two distinct nonvolatile
published globals, the section anchor, and known scalar void-call signatures.
The layout-stage planner then proves a zero-offset BSS address, the fixed-load
unsigned quotient, and a complete eight- or ten-store CTR fill. It interleaves
address setup and publication with the quotient, retains the quotient in r4,
and uses r5 for the fill cursor. The prior cursor may be a saved home or a
volatile temporary. Its old value and the replaced scratch values must be dead
on the loop's fallthrough exit; the backedge intentionally carries the recolored
cursor. Known nonvariadic void calls kill scratch lanes beyond their declared
arguments, while unknown calls retain conservative ABI liveness.

`fixed_fill_cursor_copy_style` independently records GC/1.3's logical copy and
the later builds' add-zero copy. A logical copy drops its displacement owner only
when the source symbol-order stream independently owns that array. Layout
refreshes fixup indices after cursor scheduling, and the final permutation
preserves relocation and displacement owners. Volatile or aliased publications,
nonzero/unresolved addresses, live cursor values, side entries, opaque code,
additional owners, and malformed loops prevent the rewrite. Selection is limited
to predecrement O3/O4 performance builds with scheduling on.

Canaries **2240–2245** add eight variations across six modes and fifteen builds:
fixed loads at two addresses, divisors 400 and 1000, zero and nonzero fills,
reversed publications, volatile outputs, an aliased output, and a call barrier.
All **90 objects / 720 functions** compile on baseline, candidate, and reference.
**64 functions** change at identical sizes and gain **0 → 64** exact first-fill
packets. Whole-function matches remain **0/720**. All **11,520** native cases pass
memory guards, fixed-load counts, callback-visible publications and mutations,
argument values, saved registers, stack, and return-state checks.

Of **2,460** preceding objects, **2,380** remain identical. The other **80 objects /
352 functions** gain **0 → 156** exact first-fill packets. The remaining 196
functions retain earlier BSS-base selection differences: the reference uses
separate array addresses where the candidate retains a section anchor. Whole
matches across the 1,056 functions in these objects remain **216/1,056**, with
no exact losses or function-size changes. All **5,632** affected earlier execution
cases pass using their existing models and cached reference objects. This gives
**17,392** distinct native cases for the checkpoint.

The AX/GX probe still compiles **24/24 units** on GC/1.2.5n with identical objects.
An additional GC/1.3 probe compiles **21/24 units**, with AXVPB alone changed.
Three failures reproduce identical baseline diagnostics: `__AXOutInitDSP` needs
a canonical frame owner, `__GXInitGX` rejects a conditional narrow argument to
`GXSetFieldMode`, and `GXGetTexObjMipMap` rejects a narrow return expression.
These are open project-compilation work, not regressions from this change.

Backend tests pass **1,689** with the existing nested-asm exclusion, object-stage
tests pass **33**, and version tests pass **64**. Seven new object tests cover
both fill widths, copy forms, metadata, idempotence, ownership, alias/volatility
proofs, address layout, saved and volatile cursor lifetimes, call argument counts,
and malformed/interior-entry barriers. The new profile test checks all fifteen
builds. Final recompilation reproduces every execution-tested candidate hash.

`target/later-fill-final-verification.json` binds the final compiler/harness,
555 native object entries, sample sources, execution harnesses, preceding objects,
library outcomes, and the original DOL fingerprint. These are targeted results,
not a full-corpus parity estimate. The next initializer differences are its
successive clear-loop setup; the three newly measured GC/1.3 compile failures
also provide concrete project-driven follow-up work.

## Complete early AX voice initializer matching, 2026-09-08

The real `__AXVPBInit` now matches all **512 bytes and relocations** on GC/1.1,
1.1p1, 1.2.5, and 1.2.5n. Complete initializer matches improve **0 → 4/15**,
building on the earlier 256-byte prefix. The eleven later-version objects
remain unchanged, and `__AXSetPBDefault` remains **15/15** exact. All **240**
initializer execution cases pass against baseline, reference, and original
GQPE78 game models, including every voice's callback-visible contents, clear
ranges, flush arguments, saved registers, stack, and return state.

The new machine-stage reset scheduler keeps a stable callback pointer in r3
while issuing member stores. It requires a known nonvariadic void callback,
scalar register arguments copied from saved homes, one terminal call, and a
forward body whose scratch inputs are defined on every incoming path. It moves
only the first argument copy earlier and shifts the body scratch lanes; later
argument copies retain their ABI registers. The reset owner can differ from the
callback pointer. A separate `early_reset_comparison` profile policy captures
GC/1.1p1's comparison issue slot. A terminal tag literal can move into the join
after its last dependency, without crossing incoming branch targets. Shared
instruction permutation preserves branch and relocation ownership. Unknown
calls, additional callbacks, incoming scratch values, r12 conflicts, pointer
redefinitions, side entries, fixups, and opaque instructions reject the plan.
The pass is restricted to linkage-first O3/O4 performance builds with scheduling
on; measured O2 references use a different indexed loop shape.

GC/1.1p1's existing stack-before-LR epilogue policy now also applies when a dense
`lmw` already restored the saved range. Convention-aware frame owners previously
skipped this normalization. The proof requires matching save/load slots, a full
saved range inside the frame, and the canonical unsplit return tail. The stack
release moves before the LR reload, whose offset becomes 4 from the restored
stack. Split entries and displacement/relocation owners prevent this rewrite.

Canaries **2234–2239** add eight callback/reset variations in six modes across
fifteen builds. All **90 objects / 720 shared case functions** compile on
baseline, candidate, and reference. **58 functions** change at identical sizes:
40 gain reset-prefix scheduling and volatile-register matches after normalizing
already-different saved homes; 18 change only their epilogue. Across those 58,
**28** gain reference epilogue order. Whole case-function matches remain **0/720**;
reference-only static reset helpers also remain in three later size-mode objects.
All **5,760** native cases pass guarded memory, callback mutations and argument
checks, saved registers, stack, and return-state checks. Final O2 gating changed
only four objects; their 256 cases were rerun and merged with hash-identical
results for the remaining objects.

Of **2,370** preceding objects, **2,329** remain identical. The other **41 objects /
198 functions** change only their final epilogue instructions, with identical
preceding bytes, function sizes, and relocations. These gain **0 → 198** reference
epilogue orders. Whole matches across their 446 shared functions remain **62/446**,
with no exact losses. All **3,008** affected earlier execution cases pass;
unaffected functions were not re-executed. This checkpoint covers **9,008**
distinct native cases in total.

All **24 AX/GX units** compile, with AXVPB alone changed. Backend tests pass
**1,689** with the existing nested-asm exclusion; version tests pass **63**.
Seven new scheduler tests cover profile order, argument registers, pointer
ownership, scratch dataflow across joins, dependency and entry barriers,
metadata remapping, and idempotence. Two frame tests cover complete dense
restores and rejected malformed/split tails, and one version test covers policy
selection across all fifteen builds.

`target/reset-call-final-verification.json` binds the final compiler/harness,
438 native object entries, sample sources, execution harnesses, preceding objects,
and original DOL fingerprint. These are targeted measurements, not a full-corpus
parity estimate. Later-version initializer code generation and the new samples'
saved-home differences remain open.

## Anchored cursor setup scheduling, 2026-09-08

The real `__AXVPBInit` now matches through cursor setup and its first voice-index
store: **256 bytes and relocations** on GC/1.1, 1.1p1, 1.2.5, and 1.2.5n.
These prefix matches improve **0 → 4/15**, extending the previously matching
228-byte prefix. Only the six cursor-setup instructions move; the preceding
clear loops and every instruction from byte 252 onward remain identical. The
eleven later-version objects remain unchanged. Complete initializer matches
remain **0/15**, and `__AXSetPBDefault` remains **15/15** exact.

Source strength reduction nominates array groups initialized from a retained
BSS anchor. The layout-stage planner consumes those symbolic groups after full
address expansion, validates independent saved-register destinations, and
schedules high halves before complete addresses, the zero index, and pending
low halves. The zero-offset cursor leads other complete addresses even when
source binding order puts it last. A separate `patched_cursor_setup_order`
profile policy records GC/1.1p1's measured ready-list order; permuting stores,
cursor binds, and callback arguments disproves first-use ordering as its cause.
The pass applies to linkage-first O3/O4 performance builds with scheduling on;
narrow-only groups retain their existing path.

A page-aligned wide address is already complete after `addis`. Its redundant
low add and displacement owner are removed only when the source symbol-order
stream independently retains that array. Such completed addresses participate
in the ready list rather than pending high/low pairs. The patched completed-page
packet also delays the index past the remaining low half when a preceding call
or save helper changes its ready-list context. Plans are validated together and
applied from the end, preserving surviving fixup/relocation owners and branch
boundaries as packets shrink. Interior entries, aliasing registers, mismatched
anchors, extra owners, opaque code, and unowned symbols prevent the rewrite.

Canaries **2228–2233** add nine functions in six modes across fifteen builds.
Their arrays produce one wide and three narrow addresses while source store,
binding, and argument orders vary. All **90 objects / 810 functions** compile
on baseline, candidate, and reference. **72 functions** change at identical
sizes, gaining **0 → 72** exact cursor-setup packets. Whole-function matches
remain **0/810**. All **12,960** execution cases pass guarded-memory, callback,
input-read, saved-register, stack, and return-state checks.

Of **2,280** preceding objects, **2,248** remain identical. The other **32 objects /
120 functions** contain the same anchored setup. The completed-page case removes
one instruction from **32 functions** in canaries 2121–2123 and 2127, and their
setup packets improve **0 → 32** exact matches. Eight `far_tail3` functions gain
**0 → 8** exact setups. Eighty following-fill functions gain reference setup
order; **72** also match registers exactly, while eight callback-barrier samples
retain earlier saved-home differences. No exact function matches are lost.
All **1,600** affected earlier execution cases pass; unrelated functions were
not re-executed. All **240** real initializer cases pass against reference and
original game models, for **14,800** distinct native cases in this checkpoint.

All **24 AX/GX units** compile, with AXVPB alone changed. Backend tests pass
**1,680** with the existing nested-asm exclusion, object-stage tests pass **26**,
and version tests pass **62**. Eight new object-stage tests cover profile order,
metadata remapping, completed-page ownership, multiple shrinking packets,
idempotence, dependencies, side entries, and malformed candidates. The final
compiler's newly compiled objects were checked against execution hashes; only
the changed completed-page objects needed another execution run after refinement.

`target/cursor-setup-final-verification.json` binds the final compiler/harness,
411 native object entries, sample sources, execution harnesses, preceding objects,
and original DOL fingerprint. These are targeted measurements, not a full-corpus
parity estimate. The next real-initializer differences are the callback argument's
live range, reset-store register selection, and instruction scheduling.

## Successive fixed-fill scheduling, 2026-09-08

The real `__AXVPBInit` now matches through all three clear loops: **228 bytes
and relocations** on GC/1.1, 1.1p1, 1.2.5, and 1.2.5n. These prefix matches
improve **0 → 4/15**, extending the previously matching 112-byte prefix. Only
the second and third clear loops change; the first 112 bytes and everything
after byte 228 remain identical. The eleven later-version objects remain
unchanged. Complete initializer matches remain **0/15**, while
`__AXSetPBDefault` remains **15/15** exact.

A layout-stage planner schedules complete BSS addresses after their wide
high/low halves exist. The backend carries an optional, index-free policy for
linkage-first O3/O4 performance builds with scheduling enabled. Narrow addresses
follow `mtctr` except in GC/1.1p1, where they precede it. Wide addresses put their
high half between the count literal and `mtctr`, then finish the low half after
the fill literal. This reuses the measured early scheduler profile choice.

The planner recognizes an owned address followed by a complete word-store CTR
packet immediately after another CTR loop. It reuses r3 for a disposable pointer
only when both the old pointer and r3 are overwritten before any later use on
all reachable paths. Calls, indirect transfers, returns, system exits, side
entries, inline assembly, jump tables, relocations, and extra displacement
owners prevent unsafe rewrites. The address owner's index moves with the low
half; store order, packet size, and backedge positions are preserved. Seven new
unit tests exercise both schedules, metadata ownership, idempotence, live
values, malformed packets, branching lifetime proofs, and implicit-use barriers.

Canaries **2222–2227** add ten functions in six modes across fifteen builds.
Their four 4,096-word arrays exercise both narrow and signed-low wide offsets;
a separate clear pointer models the real initializer's temporary lifetime.
Counts, loaded/register/literal inputs, nonzero fills, a call barrier, and an
observed final pointer vary independently. All **90 objects / 900 functions**
compile on baseline, candidate, and reference. **72 functions / 208 loop packets**
change without changing size; the changed packets improve **0 → 208** exact
matches, including registers, instructions, and relocation-relative positions.
Whole-function matches remain **0/900**; subsequent cursor setup still differs.

All **14,400** sample execution cases pass memory, input-read, callback,
saved-register, stack, and return-state models. GC/1.1p1's O0 `argument` now uses
a 40-byte frame, so its parameter home does not overlap the saved registers.
A sixteen-case rerun removes the inherited 32-byte-frame exception and verifies
ordinary register preservation; all other checks and cases remain unchanged.
This does not resolve the earlier sample's reference parameter-overlap bug.
All **240** full initializer cases pass against reference and original game
models, for **14,640** distinct native cases in this checkpoint.

All **2,190** preceding objects remain identical. All **24 AX/GX units** compile,
with AXVPB alone changed. Backend tests pass **1,680** with the existing nested-asm
exclusion; object-stage tests pass **18**. The final compiler was rebuilt and
all 105 sample/initializer objects recompiled; their hashes equal the executed
objects, so execution was not repeated solely for the rebuild.

`target/fill-following-final-verification.json` binds the final compiler/harness,
315 native object entries, sample sources and execution harnesses, preceding
objects, and original DOL fingerprint. Targeted measurements do not estimate
full-corpus parity. Next differences in the real initializer begin with retained
cursor setup, followed by reset-store scheduling and register selection.

## First fixed-fill setup scheduling, 2026-09-08

The real `__AXVPBInit` now matches the reference through its entire first clear
loop: **112 bytes and relocations** on GC/1.1, 1.1p1, 1.2.5, and 1.2.5n. These
prefix matches improve **0 → 4/15**, extending the previously matching 48-byte
prologue. Only the six setup instructions between byte offsets 48 and 72 move;
the prologue and every instruction after setup remain unchanged. The eleven
later-version objects remain identical. Complete initializer matches are still
**0/15**, and `__AXSetPBDefault` remains **15/15** exact.

A dedicated fixed-fill entry planner distinguishes a memory-dependent unsigned
quotient, an incoming-register quotient, and a literal publication. It traces
the quotient's selected multiply operands rather than relying on source names
or magic constants. Ready values and loaded quotients use different count,
address, and publication orders. A separate `FixedFillAddressPlacement` profile
policy captures GC/1.1p1's earlier quotient-dependent address setup; literal
publications keep the ordinary ready-value order. This policy has a fifteen-build
test and is independent of the existing division-address policy.

The pass requires a linkage-first frame, O3/O4 performance optimization, enabled
scheduling, an owned section-relative array address, and the first CTR fill.
It validates the two global stores, distinct value/pointer/count lanes, complete
word-store packet, pointer step, and loop backedge. Side entries, preceding
control flow or calls, unknown instructions, and unowned relocation/displacement
state prevent the rewrite. Only setup instructions are permuted; global stores
retain source order, and shared remapping preserves their relocations and the
array address's deferred displacement. Unit tests cover value origins, operand
and store dependencies, metadata movement, malformed fills, aliases, and barriers.

Canaries **2216–2221** add nine functions in six modes across fifteen builds.
All **90 objects / 810 functions** compile on baseline, candidate, and reference.
They vary counts (64, 80, 96, 128), source values, nonzero fill values, and a call
barrier. **64 functions** change without changing size. Their six setup operations
now appear in reference order (**0 → 64**), abstracting the differing clear-pointer
home. Whole-function exact matches remain **0/810**; saved clear-pointer placement
and later-loop schedules are still different. All **12,960** candidate cases pass
arithmetic, read-count, callback, guarded-memory, saved-register, and stack models.

Reference validation exposes GC/1.1p1's O0 parameter/save-slot overlap in the
`argument` sample: `_savegpr_26` stores r26 at new SP+8, then the parameter home
overwrites that slot. The reference restores the input into r26. A targeted
sixteen-case rerun verifies this exact outcome; other checks remain strict and
pass. Candidate preserves r26, so reproducing this reference bug in this frame
shape remains a parity task. The final execution record names the correction
and retains the unchanged cases' earlier verified results.

Of **2,100** preceding objects, **2,084** remain identical. The other **16 objects /
24 functions** gain the same reference setup order (**0 → 24**) with no exact
function losses; all **384** affected native cases pass. Only those affected
functions were rerun. All **240** real initializer cases pass against reference
and original game models. All **24 AX/GX units** compile, with AXVPB alone changed.
Backend tests pass **1,680**, with the existing nested-asm exclusion; version tests
pass **61**. Fresh distinct native coverage totals **13,584 cases**. These targeted
measurements do not estimate full-corpus parity; broader panels were not rerun.

`target/fill-entry-final-verification.json` binds the final compiler/harness,
source/model hashes, original DOL, **363 native-tested object entries**, and all
measured comparisons. Subsequent clears still differ in pointer homes and count/
address ordering; later compiler versions also need their quotient homes matched.

## Fixed-division entry scheduling, 2026-09-08

A separate entry planner now fills linkage-first prologue latency slots with a
leading fixed-address division's multiplier and address constants. After the
multiply, an optional retained section base reuses the dead multiplier register.
This produces the reference's first **48 bytes and relocations** of the real
`__AXVPBInit` in **GC/1.1, 1.1p1, 1.2.5, and 1.2.5n**: prefix matches improve
**0 → 4/15**. All bytes and relocations after that prefix remain unchanged on
every build. The eleven later-version objects are entirely unchanged, complete
initializer matches remain **0/15**, and `__AXSetPBDefault` stays **15/15** exact.

The planner runs after frame normalization and uses the existing frame-convention
policy. It requires O2 or higher, performance optimization, and scheduling enabled.
It validates the complete entry before changing it: linkage and saved-register
stores, constant-address load, multiplier pair, multiply, quotient, and optional
owned section relocation pair. Interior branch entries, unowned instructions or
relocations, displacement fixups, indirect jump tables, and assembly block the
rewrite. Saved homes must precede both initialization and the memory load.
A shared permutation helper keeps instruction-index owners attached to their
instructions. No division selection or later body schedule changes.

Register reuse requires liveness proof. The general allocator conservatively
counts every materialized argument lane as an input to a following call. A known
non-variadic void callee with scalar word arguments can instead prove a staging
lane unused; the planner models that call as a definition of the lane only in
the liveness analysis. Unknown, wide, floating, and variadic signatures retain
the conservative check. Unit tests cover partially scheduled prefixes, relocation
movement, interior entries, effects, live temporaries, late saves, and this call
prototype boundary.

Canaries **2210–2215** add twelve functions in six modes across fifteen builds.
All **90 objects / 1,080 functions** compile on baseline, candidate, and reference.
They cover several divisors, declared and cast absolute addresses, nonzero and
zero high halves, incoming arguments, a preceding-call barrier, and three/four
cursor clearing loops. **108 functions** change without changing size; exact
matches improve **35 → 119**, with no losses. All **17,280** native cases pass
volatile-read, callback mutation, guarded-memory, register, and stack checks.

The previous division slice adds **36** exact functions (**780 → 816/2,340**),
with **64** functions changed and all **37,440** native cases passing. The other
**1,920** preceding objects remain byte-identical. These are diagnostic slices,
not a corpus parity estimate. The wider older arithmetic panel and full corpus
were not rerun.

All **240** real initializer execution cases pass against reference and original
game models. All **24 AX/GX units** compile; AXVPB alone changes, while the other
23 units remain byte-identical. Backend tests pass **1,676**, with the existing
nested-asm exclusion. Fresh native coverage totals **54,960 cases**. Final objects
were rebuilt after the last validation-guard change and their hashes checked
against those native executions.

`target/divide-entry-final-verification.json` binds the final compiler/harness,
source and model hashes, original DOL, **585 native-tested object entries**, and
measured comparisons. The next initializer gaps include zero/count/store ordering
around the first clear and the later compiler versions' quotient homes.

## Fixed-address unsigned division, 2026-09-08

A separate operand selector now keeps an absolute-address word dividend in r0
and materializes the unsigned magic multiplier in another register. The existing
constant-address cache owns base lifetimes and carry-adjusted address splitting;
the division selector owns the multiply operands and scheduler-dependent load
order. It applies at O2 and above to the no-add magic sequence. Integer casts
that preserve all 32 bits are recognized, including the `(u32)__OSBusClock`
macro in the real AX initializer. Narrow, floating, wide, signed-division,
computed-dividend, and add-form division paths retain their existing selection.

Native checks exposed an older correctness bug: a fixed-address base or computed
division operand could overwrite an incoming argument before a later identity
call argument used it. Both now honor explicit register reservations, including
cache hits. Canaries **2204–2209** cover 26 functions in six modes and fifteen
builds: direct and declared absolute loads, signed-to-unsigned casts, narrowing,
several divisors and address pages, callbacks, retained inputs, and clearing and
cursor loops. All **90 objects / 2,340 functions** compile on baseline, candidate,
and reference. **1,440 functions** change without changing size. Whole-function
exact matches improve **0 → 780**, with no exact losses. The improvement is local
to this diagnostic slice, not a corpus parity estimate.

All **37,440** new candidate and reference execution cases pass arithmetic,
volatile-read counts, callback mutation, guarded memory, saved registers, and
stack checks. Baseline fails **1,440** retained-argument cases; candidate fixes
all of them, including O0. The reference's GC/1.1p1 O0 parameter home at caller
SP is checked explicitly against the input value; candidate still keeps that
input in a register. This preserves the distinction between correct execution
and exact version-specific stack behavior.

The preceding **1,920 objects** remain byte-identical. A separate older arithmetic
and absolute-address panel attempts **660 configurations**: baseline and candidate
both compile the same **556**, all byte-identical; reference compiles **657**.
The **553** jointly compiled objects retain **507/1,519** exact functions. Existing
unsupported constructs, source-encoding failures, and three reference tool/flag
failures are outside those paired measurements. No broader corpus run was made.

All fifteen real AXVPB objects compile; only `__AXVPBInit` changes. The loaded
dividend now occupies the reference's r0 on all builds, and the entire multiply
register tuple matches the four oldest builds. Later references use r4 for the
multiplier/result. All **240** initializer cases pass against reference and
original-game models. Complete initializer matches remain **0/15**, while
`__AXSetPBDefault` remains **15/15** exact. Constant hoisting through the prologue,
section-address placement, and later-build quotient homes still need work.

All **24 AX/GX library units** compile. AXVPB, GXInit, and GXPixel change; the other
21 units remain identical. The real `GXInit` passes **512** native comparisons
with reference and original game code, including clock-changing callbacks.
`GXSetPixelFmt` uses a different FIFO base temporary after reservation handling;
all **1,024** native comparisons preserve context, FIFO writes, and ABI state.
Neither complete GX function becomes exact. Backend tests pass **1,671**, with
the existing nested-asm exclusion. Fresh native coverage totals **39,216 cases**.

`target/fixed-divide-final-verification.json` binds the final compiler/harness,
canary and native-model source hashes, original DOL, **319 native-tested object
entries**, preserved objects, and measured comparisons.

## Wide BSS cursor bases, 2026-09-08

The temporary-address pass now has a separate full-width recipe. Measured
profile capabilities select it for GC/1.1, 1.1p1, 1.2.5, 1.2.5n, and **1.3**;
the transition is independent of frame convention. An offset-zero cursor can
own the section base even when it is bound last. Other addresses split into a
rounded high page and signed low displacement, omitting zero low additions.
The four oldest builds reuse already completed page cursors. GC/1.3 also shares
page expressions in volatile registers and can form a later page cursor before
its dependents. Arbitrary nearby completed addresses do not count as shared
page expressions.

The existing packet validator and rewrite machinery remain shared between
narrow and wide recipes. Full-width immediates require existing source-order
symbol ownership so removing relocations cannot alter their proven layout.
Growth remaps instruction owners and control-flow endpoints, declining any
replacement that would exceed a branch encoding. A one-temporary fallback
preserves correctness when GC/1.3 cannot retain a separate section base. New
tests cover zero-cursor ownership, page reuse, wrapping address arithmetic,
all binding permutations, temporary pressure, symbol discovery, and growing
packets near the branch limit. The version boundary has a fifteen-build test.

Canaries **2198–2203** add seven functions in six modes across fifteen builds:
all **90 objects / 630 functions** compile on baseline, candidate, and reference.
They cover binding order, three/four cursors, scalar inputs, aliasing,
initialized data, scheduling, and optimization controls. **75 functions** in
the five early builds change: 36 shrink by four bytes, 36 by eight, and three by
twelve. All 75 use the reference's shared BSS relocation form and the same
address instructions after normalizing the HA temporary register and ignoring
interleaving. Keeping instruction order, normalized address-sequence matches
improve **0 → 35**. Whole-function exact matches remain **6/630**. All **10,080**
new native cases pass callback, mutation, guarded-memory, register, and stack
models.

Of **1,830** preceding objects, **1,775** remain identical. The other **55 objects /
55 functions** adopt the reference's shared wide-address form without losing
exact functions. All **3,040** affected preceding execution cases pass. Some
biased-section functions grow by four or eight bytes, reproducing the older
compiler's repeated high-half calculations. GC/1.3's schedule-off temporary
placement and older high/low interleaving still need matching work.

Fresh native coverage totals **13,120 cases**. All fifteen AXVPB objects and all
24 AX/GX units compile unchanged. The initializer retains its 12/15 matching
home maps and exact 28-byte loop tails, with 0/15 complete initializers exact;
its prior 240 execution cases are reused after object-hash verification.
Backend tests pass **1,669** with the existing nested-asm exclusion, object-stage
tests pass **11**, and version tests pass **60**. Broader older/indexed panels
and the full corpus were not rerun.

`target/wide-bss-final-verification.json` binds the final compiler/harness,
source hashes, **465 native-tested object entries**, and measured comparisons.
The next real-project frontier is AXVPB prologue scheduling. Its GC/1.2.5n
48-byte frame and `stmw` save range already match; constant/address placement
and scratch-register choices differ. Reduced samples also retain frame-sizing
and save-slot-order gaps. Address-form gains are not complete function matches.

## Temporary BSS cursor bases, 2026-09-08

Cursor strength reduction now preserves each binding's array identity alongside
its local role. O3/O4 performance lowering carries candidate address groups to
the machine representation when no retained section anchor owns the function.
A separate object-stage pass consumes them after final BSS ordering/alignment,
before debug lowering. It shares three or more distinct bases when their
complete section offsets fit signed D-form immediates.

The pass proves a contiguous packet of absolute address pairs, checks relocation
ownership and interior control-flow entries, and uses CFG liveness to find a
free volatile GPR. It replaces the packet with one section address plus one
addition per cursor, retaining symbol-discovery fixups and remapping later
instruction owners and branch targets. Saved-register allocation and frames
remain unchanged. Unknown/initialized data, duplicate bases, wide offsets,
interfering fixups, and unavailable temporaries decline the transformation.
Three new unit tests exercise live incoming registers, branch/entry remapping,
fixup ownership, signed boundaries, register pressure, and idempotence.

Canaries **2188–2193** add six functions in six modes across fifteen builds:
**90 objects / 540 functions**, all compiling on baseline, candidate, and
reference. They vary cursor count and binding order, scalar parameters,
aliasing, initialized data, scheduling, and optimization mode. **180 functions**
change: 135 shrink by four bytes and 45 by eight. Every changed function now
uses the reference's one shared BSS HA/LO relocation pair instead of separate
symbol pairs. This measures address-form agreement; complete functions remain
**0/540 byte-exact**. All **8,640** native cases pass independent callback,
mutation, guarded-memory, register, and stack models.

Canaries **2194–2197** add **60 objects / 75 functions** around layout boundaries
and biased/wide sections. Twelve older-layout boundary cases shrink by four
bytes and adopt the reference's shared address form. The three newest builds
align the third base from `0x7ffc` to `0x8000`, correctly declining this form.
All **600** native cases pass; the 15 existing exact padding-accessor matches
remain exact. Earlier builds can also share wide addresses through an initial
cursor home; that distinct form remains future work.

Of **1,680** preceding objects, **1,560** stay identical and **120 objects /
690 functions** change. All 690 adopt the reference's shared relocation form,
shrinking by four through twenty-four bytes, without exact-function gains or
losses. All **17,040** affected preceding native cases pass. Fresh execution
coverage totals **26,280 cases**. Full AXVPB remains byte-identical in all fifteen
builds, preserving its 12/15 matching home maps and exact 28-byte loop tails;
complete initializer parity remains 0/15. Its prior 240 executions are reused
only after object-hash verification. All 24 AX/GX units compile unchanged.

Backend tests pass **1,669** with the existing nested-asm exclusion; object-stage
tests pass **6**. All **1,869** candidate objects were recompiled with the final
compiler image and matched their tested hashes. The final compiler/harness,
source hashes, comparisons, and **840 native-tested object entries** are bound
by `target/temporary-bss-final-verification.json`. Broader older/indexed panels
and the full corpus were not rerun. Prologue placement of temporary bases,
wide-address sharing, and modern two-array retained-anchor profitability remain
separate matching gaps.

## Cursor section-base lifetimes, 2026-09-08

The four early builds previously planned a saved BSS section base from source
array bindings inside the loop, even when strength reduction moved every such
binding into one-time loop setup. Their O3/O4 anchor analysis now consumes the
same reduced function already used by the modern planner. A shared helper owns
that reduction; path-sensitive call-span analysis still retains bases needed by
loop-body references or by accesses before and after calls. Initialized-data
planning, unreduced functions, O0/O2, and modern-build output are unchanged.

Canaries **2182–2187** add fourteen functions in six modes across fifteen builds:
**90 objects / 1,260 functions**, all compiling on baseline, candidate, and
reference. They cover public and file-local arrays, two and three cursors,
setup-only addresses, calls before the loop, accesses in and after the loop,
and arrays separated by 32 KiB. **80 functions** change in the four early
builds. Complete saved-register home maps at the first loop callback improve
**351 → 411**, with no losses. Whole-function exact matches remain **0/1,260**.
All **10,080** native cases pass callback, mutation, guarded-memory, volatile
clobber, saved GPR/FPR, caller-stack, SP, and LR checks.

Of **1,590** preceding objects, **1,506** remain identical and **84 objects /
384 functions** change, with no exact-function gains or losses. All **8,640**
affected preceding native cases pass. On the affected prior cursor-home panel,
complete register-map matches improve **12 → 96**, closing its 84 early-build
performance-mode mismatches. These are register-map gains, not exact functions.

The reference uses a temporary section base for three nearby setup addresses;
that sharing remains unimplemented. Removing the saved anchor therefore shrinks
32 new functions by eight bytes but grows 48 by twenty or twenty-eight bytes,
including changes between helper-based and individual register saves. Public
and static arrays show the same behavior in these probes. Modern two-array
post-loop cases still retain an unnecessary base in the candidate; address
sharing profitability needs separate analysis from anchor lifetime.

All fifteen full AXVPB objects and all 24 AX/GX translation units compile and
remain byte-identical to the preceding compiler. AXVPB retains its **12/15**
matching home maps and exact 28-byte loop tails, with **0/15** exact complete
initializers. The prior **240** initializer executions are reused after object
hash verification. Fresh native coverage totals **18,720 cases**. Backend tests
pass **1,669**, retaining the existing nested-asm exclusion. Broader older and
indexed panels and the full corpus were not rerun.

`target/anchor-span-final-verification.json` binds the final compiler/harness
fingerprint to source and object hashes, **552 native-tested object entries**
(including the unchanged initializer evidence), and the measured comparisons.

## Global-array cursor home priorities, 2026-09-08

AXVPB's retained pointers previously followed the logical index in saved-register
allocation. The global-array strength reducer now retains source-role facts:
which local is the logical index and the leading binding order of the cursor
locals. Its rewritten executable function remains separate from those facts.
The writable-section anchor analysis consumes the same rewritten function as
before. A separate home-priority planner uses the roles after ordinary liveness
and deferred-home planning, without reconstructing them from lowered comma
expressions or changing register interference.

For O3/O4 performance mode, cursors rank first in leading source-binding order,
followed by eager scalars, the logical index, and surviving parameters. Retained
section bases keep their preceding saved homes. Reversing declarations, later
use order, or use counts does not reorder the cursors; reversing their leading
bindings does. The planner requires one reduced cursor group, distinct deferred
homes, a complete role mapping, and no parameter-home sharing. Other plans
retain their existing preferences. O2 and size mode still need their different
indexed-address representations.

Canaries **2176–2181** contain nine functions in six modes across fifteen builds:
all **90 objects / 810 functions** compile on all three sides. They vary binding
and declaration order, use order/counts, incoming and eager scalar values,
post-loop uses, and explicitly initialized cursors. **360 functions** change,
with unchanged instruction counts. Capturing the saved GPRs at the first callback
shows **0 → 243 complete reference home-map matches**. The other 117 changed
cases retain an extra section anchor: early builds in ordinary loops, and later
builds when arrays are referenced after the loop. Whole-function exact matches
remain **0/810**; these home-map matches are not byte-exact function matches.

All **12,960 new execution cases** pass against the reference and independent
memory/callback models. Checks include scalar overflow values, callback
arguments, mutation/clobbers, guarded arrays, callee-saved GPRs/FPRs, SP, and LR.
Of **1,500** preceding objects, **1,215** remain identical; **285 objects / 1,605
functions** change register allocation without changing instruction counts or
losing exact matches. All **28,560** affected earlier execution cases pass.
The older **1,110** and recent **2,626** objects remain identical, with **89**
unchanged recent failures. All **1,674** indexed outcomes remain unchanged
(1,114 compiled, 972 known exact).

Full AXVPB changes only `__AXVPBInit` across fifteen builds. Its retained home map
and its **28-byte loop tail now match exactly in 12/15 builds**, up from zero.
This advances the preceding checkpoint's register-normalized tail matches to
actual tail bytes. The three newest builds still need a different synthesized
offset representation. All **240** initializer cases pass against the reference
compilers and original game binary. The complete initializer remains **0/15
byte-exact**; instruction counts are unchanged. All 24 AX/GX translation units
compile, and the other 23 objects are identical.

Total native coverage is **41,760 cases**. Backend tests pass **1,669**, retaining
the existing nested-asm inline exclusion. The full corpus was not rerun.
`target/cursor-rank-final-verification.json` binds the final compiler/harness
fingerprint to **1,170 native-tested object entries**, the captured home maps,
and preceding-object hashes.

## Counted-loop latch scheduling, 2026-09-08

The AXVPB initializer exposed a shared scheduling gap: the induction comparison
followed all pointer steps instead of issuing near the induction update. A
separate late scheduler now canonicalizes independent post-call updates and
places the constant-bound comparison according to the version and processor.
It requires a backward conditional edge, distinct in-place integer updates,
and a positive induction step. It rejects alternate packet entries, dependent
updates, and symbolic instruction owners, and uses the shared permutation API
for durable instruction metadata. O0/O2/O3, descending loops, and register-bound
comparisons retain their existing owners.

For `-proc gekko` or `7400`, GC/1.1 and GC/1.2.5[n] compare immediately after the
induction update. GC/1.1p1 and later builds issue one independent step first,
rotating the first remaining step to the end. Disabling scheduling selects the
immediate comparison. Processor selection is now an explicit invocation fact,
separate from the existing scheduling pragma: `603e`, `604`, and `750` select the
interleaved order across the measured builds. Omitting `-proc` selects immediate
comparison except in GC/1.1p1. An explicit `scheduling 7400` pragma does not
replace this processor selection in the measured loop packets. Profile tests
and a driver last-wins test preserve these boundaries.

Canaries **2170–2175** contain ten functions in six modes across fifteen builds:
**90 objects / 900 functions** compile on baseline, candidate, and reference.
They cover signed/unsigned indices, equality and dynamic bounds, descending
loops, pointer termination, one through three cursors, unequal strides, global
arrays, aliases, and callbacks. **315 functions** change. Comparing loop-tail
opcodes and immediates while ignoring physical register numbers gives
**0 → 300 matching tails** among them. The fifteen size-mode global-array cases
still differ because the reference uses one shared array offset. Whole-function
exact matches stay **2 → 2**, with no losses; normalized tail matches are not
byte-exact function matches.

All **14,400 new native cases** pass on all three sides. The model checks
callback arguments and memory, callback clobbers/mutation, aliasing, guarded
memory, SP/LR, and callee-saved registers. It preserves **16** GC/1.1p1 O0 cases
where a single-use end pointer overwrites the saved r30 slot. A separate panel
covers six processor/pragma configurations across fifteen builds: all **90**
normalized three-cursor tails match, and **14,400** additional candidate/reference
execution cases pass.

Of **1,410** preceding objects, **1,170** remain identical. The **240 changed
objects / 1,503 changed functions** retain all exact matches. Every changed
function preserves its instruction multiset and relocations; all **27,840**
affected earlier execution cases pass. The older **1,110** and recent **2,626**
objects remain identical, with **89** unchanged recent failures. All **1,674**
indexed outcomes remain unchanged (1,114 compiled, 972 known exact).

Full AXVPB objects change only `__AXVPBInit` across fifteen builds. Its normalized
loop-tail order now matches **12/15**, up from zero; the three newest builds
still use a different order of synthesized offsets. All **240** initializer
cases pass against both reference compilers and the original game binary.
Instruction counts are unchanged and the initializer remains **0/15 byte-exact**.
All ten AX and fourteen GX translation units compile; the other 23 objects are
identical. Total native coverage for this checkpoint is **56,880 cases**.

Backend tests pass **1,667** with the existing nested-asm inline exclusion;
version tests pass **59**, plus the focused processor-option driver test.
The full corpus was not rerun. `target/loop-latch-final-verification.json` binds
the final compiler/harness fingerprint to **1,215 native-tested object entries**,
the preceding-object hashes, and the instruction-permutation checks.

## Dependent word call inputs, 2026-09-08

Calls such as `observe(p, *q)` now preserve incoming register values before
placing overlapping ABI arguments. A separate dependency planner schedules
pure word expressions and breaks register-leaf cycles with virtual snapshots.
O0, disabled scheduling, and modern volatile calls instead snapshot endangered
inputs and evaluate in source order. The four early 2.3.3 profiles reproduce
their measured optimized volatile-load reversal through a version policy.
Existing ABI classification and expression evaluation remain shared; C++
reference address recovery and other argument families retain their owners.
Typed argument evaluation also preserves signed-byte promotion, including
pointers whose type identity survives only in a frame slot.

GC/1.1p1 O0's source-home planner now also covers direct pure word calls. This
preserves the original single-use parameter alias at SP+8. With no retained
registers, the eight-byte frame places that image over the caller's backchain.
All-single-use indexed inputs can consequently load through an integer index
as a pointer. These are modeled reference bugs, including the fault address,
faulting load, argument register state, and caller-stack overwrite.

Canaries **2164–2169** contain sixteen functions across six modes and fifteen
builds. All **90 candidate and reference objects** compile. The baseline fails
every whole source; isolating its functions yields **180/1,440** compilations.
The candidate recovers the other **1,260 function cases**. Exact function
matches rise **44 → 158**, with no losses: **114 new exact matches**. Of the
180 previously compiling cases, 174 remain identical and six GC/1.1p1 O0
functions become exact. The preceding reversed-pointer loop canaries
**2159–2163** also recover all **71** failures; their loop allocation and
scheduling still differ from the reference.

All **23,040 new native cases** match the reference and the independent model,
including volatile read counts/order, aliases, signed-byte and halfword loads,
callback mutation/clobbers, callee-saved state, and guarded memory. They include
**576 caller-stack corruption cases** and **48 indexed-load faults**. All
**19,200** preceding source-home execution cases pass, including the recovered
loops and the previously verified original bugs: **42,240 native cases** total.

Regression checks preserve all **1,249** previously compiling objects in the
preceding panel; its 71 recovered objects bring compilation to **1,320/1,320**.
The older **1,110** and recent **2,626** objects remain identical, alongside
**89** unchanged recent failures. All **1,674** indexed outcomes remain unchanged
(1,114 compiled, 972 known exact). Ten AX and fourteen GX translation units
compile unchanged. Full AXVPB objects remain identical across fifteen builds,
retaining the preceding **240** initializer execution checks. The initializer
is still **0/15 byte-exact**; register allocation and instruction scheduling
remain the project frontier.

Backend tests pass **1,665**, with the existing nested-asm inline exclusion;
version tests pass **57**. The full corpus was not rerun.
`target/call-input-final-verification.json` binds the compiler/harness fingerprint
to **559 native-tested object entries**, preceding-object hashes, and the fifteen
unchanged full AXVPB objects.

## Source reference counts and mutable loop homes, 2026-09-08

GC/1.1p1 O0 register-home priority depends on source occurrences that executable
AST desugaring previously erased. `sum += i` contains one occurrence of `sum`;
`sum = sum + i` contains two. They have identical executable assignments but
can receive different reference registers. The parser now records bound
variable token sites, including initializer destinations and excluding
uninitialized declarations and member/type names. Sites deduplicate parser
replays, and block-shadow bindings remain distinct. These facts pass through
the translation unit and the existing source-facts API, leaving executable
expression shapes unchanged.

The shared-spill loop planner ranks locals and retained parameters together by
descending source count. Ties use local first-definition order, then reverse
parameter declaration order. This refines the preceding checkpoint's simpler
local-first rule. Counts are unweighted by runtime loop frequency. C++,
parse-time inline substitutions, and incompletely tracked local bindings omit
the facts. Backend inline candidates also omit them: expansion can change
multiplicity without changing variable names. Missing facts retain the earlier
AST-home policy. An older inline-macro regression caught and verified this
boundary during development.

Mutable loop values also keep their planned homes through postfix updates.
Previously `*p++` could put the updated pointer in a new lane while the back
edge continued reading the old physical home, repeatedly writing one address.
The shared postfix emitter now respects the existing loop-carried-home state,
for both consumed-address updates and value-producing steps. Postfix snapshots
retain the original `mr` copy spelling through final scratch-copy normalization.

Canaries **2154–2158** contain fifteen functions in five modes across fifteen
builds. All **75 objects** compile on all three sides. Canaries **2159–2163**
isolate a reversed-pointer call-marshalling gap: each reference compiles, but
baseline and candidate compile only the four GC/1.1p1 O0 cases. The other
**71 failures** remain explicit work items, where the first argument would
overwrite a register needed by a later dereference. Across the **79 compiled
objects / 1,129 functions**, exact matches rise **4 → 64**, with no losses.
Only **60 GC/1.1p1 O0 functions** change. Eight preceding `odd_frame` and
`even_frame` cases also become exact, for **68 new exact matches** overall.

All **18,064 new native cases** pass against the reference and an independent
memory/callback model. Baseline fails **704** cases due to its loop-home bugs.
The panel varies pointer aliasing, accumulator and counter use counts,
assignment/step spellings, declaration order, frame parity, and save modes.
It checks callback arguments, callback clobbers/mutation, guarded memory,
callee-saved GPRs/FPRs, SP, and LR, including **192** original shared-slot
corruption cases. All **12,000** preceding loop cases pass on all three sides,
retaining their **512** verified original-bug cases, including fault behavior.

Regression checks retain **1,110** older compiled objects, **2,626** recent
objects plus **89** unchanged failures, and all **1,674** indexed outcomes
(1,114 compiled, 972 known exact). Of **1,170** preceding objects, **1,166**
are identical; the four changed objects contain only the eight new exact
frame functions and are covered by execution above. All ten AX and fourteen GX
translation units compile unchanged. Full AXVPB objects are identical across
fifteen builds and retain the preceding **240** initializer execution checks;
the initializer remains **0/15 byte-exact**.

Backend tests pass **1,663**, retaining the existing nested-asm inline exclusion.
Parser tests pass **411** with two preexisting failures excluded; an independent
checkout of `2556f55d` reproduces both failures (409 pass there). They are
`retains_brace_initialized_aggregate_image_from_discarded_inline` and
`recovers_friend_bearing_layouts_and_expression_template_arguments`.
The full corpus was not rerun. `target/home-rank-final-verification.json` binds
the final compiler/harness fingerprint to **462 native-tested object entries**,
the preceding-object hashes, and the fifteen unchanged full AXVPB objects.

## Shared parameter images in GC/1.1p1 O0 loops, 2026-09-08

The existing GC/1.1p1 unoptimized shared-parameter-spill policy now covers
call-bearing word/pointer loops through the shared CFG and statement emitter.
A separate source-home planner assigns multiply used values to descending saved
registers and single-use parameters to the original shared SP+8 slot. Uses are
syntactic: reading a parameter once inside a loop does not promote it merely
because that read executes repeatedly. Local homes follow first-definition
order; all parameter images precede local initializer evaluation. Frame saves
follow the measured individual/helper/multiple-register modes. The O0 lowering
retains the initial condition edge even for known trip counts.

Canaries **2149–2153** cover ten functions in five modes across fifteen builds
(O0, scheduling disabled, both explicit save modes, and O2). All **75 objects**
compile on baseline, candidate, and reference. Only GC/1.1p1 O0 changes:
**36 functions**, with exact matches rising **0 → 24 / 750** and no losses.
Seven more functions become exact in preceding canaries 2088 and 2146, for
**31 new exact matches** overall. The older inline `dual_pointer` case now
reproduces the original saved-r30 overwrite but is not yet byte-exact.

All **12,000 new native cases** pass against the reference behavior. These
include **512 original-bug cases**: 480 returning cases with overwritten saved
registers, and 32 faults where aliased pointer/condition images make the original
store through address 1. Fault checks verify the faulting store, relevant GPRs,
SP/LR, unchanged memory, and absence of a callback. Copied-pointer initializers
also observe the already-overwritten parameter image. Baseline disagrees with
these 512 bug cases. Literal scheduling preserves the original value at the
fault, as well as ordinary store results.

Earlier affected panels pass **14,400** prefix-fill, **1,728** unrolled-fill,
and **112** inline-pointer cases. This reproduces their previously recorded
**160 original-bug cases** (80 prefix, 64 conditional fill, 16 inline pointer).
These historical gaps are closed for the measured families; unrelated shared
spill and volatile-pointer gaps remain. The new loop planner deliberately
rejects address-taking, arrays, unsupported control flow/types, preexisting
frame slots or data anchors, and assembly bodies.

Regression checks preserve **1,110** older objects, **2,626** recent objects
with **89** unchanged failures, and all **1,674** indexed outcomes (1,114
compiled, 972 known exact). Of **1,095** preceding objects, **1,092** are
identical; the three changed GC/1.1p1 O0 objects are covered by the native panels
above and lose no matches. All ten AX and fourteen GX translation units compile
unchanged. Full AXVPB objects remain identical across all fifteen builds,
retaining their preceding **240** initializer execution checks. Backend tests
pass **1,663** with the existing nested-asm inline exclusion; version tests pass
**56**. The full corpus was not rerun.

`target/spill-loop-final-verification.json` binds the final compiler/harness
fingerprints to **501 native-tested object entries**, the preceding-object
hashes, and the fifteen unchanged full AXVPB objects. Final compilation
reproduces the tested candidate objects. Broader parity remains incomplete;
indexed and canary counts are targeted diagnostics.

## Retained prefix values in fixed fills, 2026-09-08

The fixed-fill loop expander now retains a matching literal from a dominating
straight-line prefix. The prefix's scratch definition and its consumers become
one virtual value, which also feeds the expanded fill. Allocation preserves that
value while issuing the CTR source. Calls, scratch redefinitions, symbolic
literal ownership, alternate entries, and a live outgoing scratch reject reuse.
Existing expansion factors, remainder stores, counter/pointer exits, and version
policies are unchanged.

BfBB `__AXVPBInit` now reuses the zero stored to `__AXRecDspCycles` for its first
clear loop. The oldest four builds shrink **516 → 512 bytes**, and the other
eleven shrink **524 → 520 bytes**. The older/middle twelve now have the same
instruction count as their references. This is still **0/15 exact**: register
assignment and scheduling differ, and GC 3/Wii reference initializers retain a
different offset-loop layout and 548-byte size. Only the initializer changes in
each full-source AXVPB object; `__AXSetPBDefault` remains **15/15 exact**. All
**240 full-initializer comparisons** pass against baseline, fresh references,
the original GQPE78 executable, and the full memory/callback model.

Canaries **2143–2148** add ten functions in six modes across fifteen builds
(default, O2, scheduling disabled, O0, debug, and size optimization). All **90
objects** compile on every side. The six positive families change in **360
functions**: global zero/nonzero fills, parameter fills, ordered writes,
remainder packets, and a returned cursor. Calls, different literals, scratch
loads, and a conditional prefix retain baseline output. Exact matches rise
**12 → 24 / 900**, with no losses.

All **14,400 candidate and baseline native cases** pass. Reference execution has
no unexpected differences after independently verifying **80 GC/1.1p1 O0
parameter-slot bugs**. In 72 cases the saved r30 slot is overwritten by a
parameter and restored incorrectly. Eight true-arm cases of `conditional`
alias the pointer and condition slots, then attempt the store through address
1; the emulator stops at that exact store with memory unchanged. The candidate
does not reproduce these original bugs yet. These are recorded parity gaps,
not successful ABI matches. The panel checks memory guards, aliasing, store
order, callback arguments/mutation/clobbers, returned pointers, saved registers,
SP, and LR.

Regression panels preserve **1,110** older compiled objects, **2,626** recent
objects plus **89** unchanged failures, all **1,674** indexed outcomes (1,114
compiled, 972 known exact), and **1,005** preceding objects from 2076–2142.
All ten AX and fourteen GX translation units compile under the existing
GC/1.2.5n library flags; only the AXVPB initializer changes. Backend tests pass
**1,661**, retaining the existing nested-asm inline-test exclusion. The full
corpus was not rerun. `target/fill-prefix-final-verification.json` binds the
compiler/harness fingerprints to **315 native-tested objects**, the verified
reference-bug records, and the preserved-object hashes. Final compilation
reproduces the tested candidate objects.

## Callback-free member-store guards, 2026-09-08

The shared leaf CFG lowerer now admits a leading store run followed by a guard,
including a single store or assignment chain in that guard. A chain is one
semantic statement, so the previous multi-statement admission check rejected
these bodies even though the emitter could handle them. The change uses the
existing lowering, liveness, return handling, and member-value graph. Dedicated
store schedules retain priority; calls, frame requirements, unsupported locals,
and unsupported control flow retain their existing eligibility checks.

This resolves the callback-free reduction failure recorded in the preceding
checkpoint. Canaries **2138–2142** contain nine functions in five modes across
fifteen builds. All **75 translation units** now compile, compared with none on
the previous compiler. Compiling each function separately establishes **450
newly supported variants** and **225 previously compiled variants whose emitted
functions are unchanged**. The recovered forms cover a guarded assignment
chain, nonzero values, ordered stores, distinct values in the arm, a single
store, and a result computed after the guard. The already supported forms cover
if/else, an intervening load, and nested guards.

The candidate produces **40/675 exact function matches**, including the eight
previously compiled exact matches. All **10,800 candidate/reference native
comparisons** pass, checking both branch outcomes, aliased objects, ordered
volatile writes, full memory guards, returned values, saved GPRs/FPRs, SP, and
LR. No assembly schedule is hard-coded for these new forms; the remaining
matches are still limited by instruction selection, allocation, and scheduling.

Regression panels retain **1,110** older compiled objects, **2,626** recent
objects plus **89** unchanged failures, all **1,674** indexed outcomes (1,114
compiled, 972 known exact), and **930** preceding objects from 2076–2137. All
ten AX and fourteen GX translation units compile and are byte-identical under
the existing GC/1.2.5n library flags. All fifteen full AXVPB objects are also
byte-identical to the preceding checkpoint's native-tested candidates; their
240 initializer comparisons were reused by verified object hashes. The AX
initializer remains **0/15 exact**, with the first clear-loop zero and other
allocation/scheduling work pending. Backend tests pass **1,658**, retaining
the existing nested-asm inline-test exclusion. The full corpus was not rerun.

`target/leaf-member-final-verification.json` binds the final compiler/harness
fingerprints to **150 freshly native-tested objects**, **15 identical prior
initializer objects**, and the preserved-object hashes. Final compilation
reproduces the tested objects. Scripts/results are under `target/leaf-member-*`
and `target/*leaf_member*.py`.

## Member constants in dominated store arms, 2026-09-08

The member-value graph can now extend an existing constant into an immediately
following guarded store arm. The arm must have one literal definition followed
only by stores, with no entry bypassing the graph's value definition. Reads,
calls, symbolic ownership, and a live outgoing r0 prevent reuse. The graph's
virtual value acquires the longer lifetime before allocation; no fixed physical
register is reserved, and all stores retain their order and multiplicity.

BfBB `__AXVPBInit` now reuses its reset zero for the final-voice pointer stores.
All fifteen builds lose one instruction: the oldest four shrink **520 → 516
bytes**, and the other eleven shrink **528 → 524 bytes**. Only the initializer
changes in each full-source AXVPB object. All **240 full-initializer comparisons**
pass against baseline, fresh references, the original GQPE78 executable, and
the complete memory/callback model. The initializer remains **0/15 exact**;
`__AXSetPBDefault` stays **15/15 exact**. The older/middle references still have
one fewer instruction, as well as different allocation and scheduling. The
remaining redundant zero is in first clear-loop setup. GC 3/Wii retain a
separate offset-loop layout in the reference.

Canaries **2133–2137** add eight functions in five modes across fifteen builds.
All **75 objects** compile on baseline, candidate, and reference. Four families
change in **240 functions**, each removing one instruction: a simple guarded
tail, an if/else tail, a nonzero retained constant, and volatile stores. Calls
before the condition, a branch bypassing initialization, an intervening load,
and a different literal retain baseline output. Exact matches remain **8/600**
with no losses. All **9,600 native comparisons** pass on all sides, including
both branch outcomes, aliased objects, volatile store order, complete memory
guards, callback mutation/clobbers, and saved registers/SP/LR.

The preceding **855 objects** (2076–2132) are byte-identical. Older/recent panels
retain **1,110** and **2,626** compiled objects plus **89** unchanged failures;
all **1,674** indexed outcomes remain unchanged (1,114 compiled, 972 known
exact). All ten AX and fourteen GX translation units compile under the existing
GC/1.2.5n library flags; only the AXVPB initializer changes. Backend tests pass
**1,658**, with the existing nested-asm inline test excluded. The full corpus
was not rerun. `target/member-guard-final-verification.json` binds the compiler
and harness fingerprints to **270 native-tested object hashes** and the
preserved-object hashes. Final compilation reproduces the tested objects.

Reducing the examples also exposed an existing leaf-lowering limitation:
removing the final callback from the simple guarded sample yields the
`leading store before a trailing if` diagnostic in `body/driver.rs`. These
canaries retain a post-guard callback to exercise the real initializer's
structure; supporting the callback-free form was pending at this checkpoint
and is resolved by the callback-free checkpoint above.

## Member values across independent stores, 2026-09-08

The existing member-value graph now shares constants and interior pointers
across intervening stores whose source already lives in another register.
Those stores retain their source, position, and multiplicity. Allocation sees
their complete live ranges alongside the shared virtual values. A changed
member base ends the run, leaving the next run's leading literal available to
its own planner. Calls, reads, control flow, symbolic fixups, and live r0 exits
retain the existing region boundaries.

This composes the caller's interior-pointer store with the inlined reset in
BfBB `__AXVPBInit`. Both observable stores remain, but their address is computed
once. The oldest four builds shrink **524 → 520 bytes**; the other eleven shrink
**532 → 528 bytes**. Only that initializer changes in each full AXVPB object.
All **240 full-initializer comparisons** pass against baseline, fresh references,
the original GQPE78 executable, and the complete memory/callback model. Exact
initializer matching remains **0/15**; `__AXSetPBDefault` stays **15/15 exact**.
The older/middle references still have two fewer instructions and different
allocation/scheduling. GC 3/Wii still use a different offset-loop layout.

Canaries **2128–2132** add eight functions across five modes (default, O2,
scheduling disabled, O0, debug) and all fifteen compiler builds. All 75 objects
compile on baseline, candidate, and reference. Exact function matches rise
**74 → 278 / 600**, with no lost exact matches. Four families change in 240
functions: leading address reuse, volatile stores, alternating independent
stores, and storing the member base itself. Other-base aliases, pointer
rebinding, callback barriers, and guarded runs retain baseline output. All
**9,600 native comparisons** pass on every side, including aliased inputs,
full memory guards, ordered volatile writes, callback mutation/clobbers, and
saved-register/stack/link-register checks.

Focused regression checks preserve **1,110/1,110** older compiled objects,
**2,626** recent objects plus **89** unchanged failures, **1,674** indexed
outcomes (1,114 compiled and 972 known exact), and **780** preceding objects
from canaries 2076–2127. All ten AX and fourteen GX translation units compile
with the existing GC/1.2.5n library flags; only AXVPB changes, and only its
initializer. Backend tests pass **1,655**, retaining the existing nested-asm
inline-test exclusion. The full corpus was not rerun.

`target/member-prefix-final-verification.json` binds the final compiler and
harness fingerprints to 270 native-tested objects and the preserved-object
hashes. The final binary reproduces the native-tested candidate objects.
Reproduction scripts and detailed results are under `target/member-prefix-*`
and `target/*member_prefix*.py`.

## Modern cursor-loop BSS anchors and deferred-address ownership, 2026-09-08

The newer profiles now retain a shared BSS base across a normalized array-cursor
loop when later code still references that section. BfBB `__AXVPBInit` shrinks
**548 → 532 bytes** on eleven builds; the oldest four remain 524 bytes. The
middle eight now save six GPRs, as the reference does, instead of independently
materializing each array from a five-GPR frame. The GC 3/Wii reference still
uses a different offset-loop layout and a wider saved range. Reference sizes
are 512/520/548, and the initializer remains **0/15 exact**. The standalone
`__AXSetPBDefault` retains **15/15 exact**.

All **240 full-initializer comparisons** pass across baseline, candidate, fresh
references, the original GQPE78 executable, and the full-memory model. Only the
initializer changes in the eleven affected full-source objects; the four older
objects are identical. Supplementary full-source reference flags continue to
remove `-W err` on every side. Three redundant value definitions and instruction
ordering remain visible against the older/middle references; equal instruction
counts alone will not prove a match.

The planner is now named `writable_section_anchor`, reflecting its shared
role across generations. Existing legacy data/BSS and modern polling-string
policies stay intact. The modern cursor path runs the existing array-induction
normalizer on the effective inline-expanded body, then applies the existing
path-sensitive reference/call analysis. A base is retained only if qualifying
full-BSS references still span a call after loop setup has been hoisted.
Setup-only references do not acquire another saved GPR. Initialized data and
small-data objects are excluded from the BSS set. Canonical unit layout still
owns the actual offsets, including addresses crossing 32 KiB and 64 KiB.

The boundary probes also exposed an existing frame-scheduling bug. Moving an
entry argument or rotating the linkage prefix updated relocations but left
deferred BSS displacements on old instruction indices. A leading callback at
O0/O2 could therefore make address finalization find a register-save instruction
instead of its address producer. Both frame-entry operations now use the common
machine-function remapper for relocations, deferred displacements, and numeric
control-flow owners. Tests cover ownership when a constant argument moves over
an address and when the frame prefix rotates. The finalizer's diagnostic now
includes the offending instruction if this invariant is violated elsewhere.

Canaries **2121–2127** cover retained two/three-array bases, setup-only references,
a callback before loop setup, an initialized-data tail, and a small-data array.
The full-BSS arrays cross signed-low and full-64-KiB displacement boundaries.
The seven modes are O4, inline saves, helper saves, O2, schedule-off, O0, and
debug on all fifteen builds. Compilation improves **97 → 105 / 105 objects**;
the eight recovered cases are the four oldest builds at O0/O2. All **630
candidate/reference functions** compile. Exact counts remain **0/630**, with
220 previously compiled functions changing. The setup-only and initialized-data
negative cases keep their previous output. All **5,040 candidate/reference
native comparisons** pass; the baseline's 4,656 compilable comparisons also
pass. Checks cover complete array/guard memory, callback arguments and memory
effects, saved GPR/FPRs, stack, and return state.

Regression panels retain **1,110** older and **2,626** recent object bytes, with
the same 89 recent compilation failures. All **1,674** indexed results remain
unchanged (1,114 compiled, 972 previously exact). **675** preceding BSS, fill,
split-address, array-cursor, inline-pointer, and cursor-frame objects are
byte-identical to their validated predecessors. All ten AX and fourteen GX
sources compile unchanged under the existing GC/1.2.5n library flags. Backend
checks: **1,653 passed**, with the existing nested-assembly inline test excluded;
all **3** object-address finalization tests pass. The full corpus was not rerun.

Local evidence: `target/cursor-anchor-final-verification.json` binds compiler
and harness fingerprints, 352 execution-tested object hashes, and 675 preserved
objects. `target/verify_cursor_anchor_final.py` checks those bindings. Full-source,
canary, native, and regression results live under `target/cursor-anchor-*`.

## Dense cursor-loop frames and explicit GPR save modes, 2026-09-08

BfBB `__AXVPBInit` now selects a contiguous GPR save range for its normalized
array cursors. Code shrinks **564 → 524 bytes** on the oldest four builds and
**572 → 548** on the other eleven. Reference sizes remain 512/520/548;
matching size on GC 3/Wii is not matching output. The initializer is still
**0/15 exact**, and the standalone `__AXSetPBDefault` remains **15/15 exact**.
Only the initializer changes within each full AXVPB source object.

The selected legacy frame grows **40 → 48 bytes**, matching the reference's
stack allocation and `stmw r26,24(r1)` / `lmw r26,24(r1)` save image. The newer
candidate frames remain 32 bytes and use save/restore helpers by default.
They still retain five GPRs rather than the reference's six: shared BSS-anchor
selection, cursor/index home order, repeated value materialization, and
scheduling remain matching work. All **240 native comparisons** pass across
baseline, candidate, fresh references, the original GQPE78 executable, and the
full-memory model, including nonvolatile GPR/FPR preservation and callback
clobbers. Supplemental full-source flags still remove `-W err` on every side.

Reduced references establish a five-live-GPR threshold for these optimized
cursor-loop frames. The existing generic frame selector waited until nine
unless inline multiple-register saves were explicitly enabled. The normalized
cursor owner now selects the existing dense-frame path at O3+ for five through
eighteen saved homes, with no automatic array/aggregate, addressable scalar
frame, or saved floating homes. Other frame owners keep their established
admission rules. Legacy cursor frames reserve a minimum of eight bytes per
saved GPR, rounded to 16 bytes; the minimum does not shrink an existing frame.

The command-line model now distinguishes an omitted `-use_lmw_stmw` from an
explicit override. The cursor-frame behavior resolves omission to inline
`stmw/lmw` on linkage-first builds and `_savegpr_N` / `_restgpr_N` calls on newer
builds; explicit `on` or `off` wins on all fifteen builds. Existing owners still
consume their established flag policy. Cursor helper saves execute before any
anchor initializer can acquire a saved register. Both save sites share one
emitter for the ABI's r11 caller-stack setup and helper relocation.

Canaries **2114–2120** add one through eight parallel array streams, a live
parameter, and an eager volatile-source local across a callback. The seven
modes are default O4, inline saves, helper saves, O2, schedule-off, O0, and debug.
All **105 objects / 1,050 functions** compile on baseline, candidate, and all
references. **435 functions change**, but exact counts remain **0/1,050**:
these probes expose remaining allocation and scheduling differences. All
**16,800 native comparisons** pass on every side, checking complete array and
guard memory, callback arguments and effects, saved registers, and return state.

Regression checks preserve **1,110** older compiled objects, **2,626** recent
compiled objects and 89 existing failures, and all **1,674** indexed statuses
and object comparisons (1,114 compiled, 972 previously exact). **570** preceding
BSS, fill, split-address, array-cursor, and inline-pointer objects are identical
to their validated predecessors. All ten AX and fourteen GX sources compile;
nine AX objects and all fourteen GX objects are unchanged. Tests: **1,652**
backend passes with the existing nested-assembly inline test excluded, **56**
version-policy passes, and the command-line save-mode test passes. No full
corpus rerun was performed.

Local evidence: `target/dense-cursor-final-verification.json` binds compiler
and harness fingerprints, 360 execution-tested object hashes, and 570 preserved
objects. `target/verify_dense_cursor_final.py` checks those bindings; full-source,
canary, native, and regression results live under `target/dense-cursor-*`.

## Pointer-call composition and interior member-value reuse, 2026-09-08

Automatic statement inlining now accepts pointer variables that change between
calls. Private pointer locals and parameters can be substituted within one
inline instance; globals and address-escaped locals use a hygienic call-time
snapshot. Pointer casts retain the same identity. Callee parameter writes still
require materialization. The existing recursive effect analysis now supports
both write-or-escape checks and escape-only checks, including nested effects
under address-of and postfix expressions.

This preserves an observed MWCC bug: when the reduced volatile-pointer helper
is inlined, its argument is reread at each use. The references perform **704
pointer reads** across 64 iterations, versus the baseline's 128. All tested
profiles do this except **Wii/1.0 O0**, which keeps the call. The candidate now
reproduces those 704 reads; Wii O0 still needs its different inline threshold.
Do not replace this behavior with C's normal single argument evaluation.

The member-value owner also composes within straight-line instruction regions,
sharing immediate constants and one interior pointer across stores before
allocation. It preserves every store and its order, including volatile and
chained writes. Live outgoing r0, interior branch entries, symbolic fixups,
opaque assembly, and unsupported value/base shapes reject the region. Existing
version policies continue to choose value issue order. Guarded initializers in
canaries 2068–2071 gain **45 exact matches: 521 → 566 / 765**; their native
panel passes **97,920 comparisons** without mismatches.

Two supporting fixes came from pointer-rebinding probes. Constant global-array
addresses can now be materialized into a virtual address register before being
copied to r0. The linkage-first callback-publication scheduler now proves that
both renamed producers die at the rewritten consumers and that no incoming
branch enters the moved region. Previously it could rename a captured global
pointer's first store while leaving a later store attached to the old register.

Canaries **2104–2113** cover changing and returned pointers, volatile pointer
reads, ordered stores, callbacks that clobber registers and memory, scalar
pointers, overlapping pointer arguments, and rebinding through globals and
escaped locals. They span O4, O2, schedule-off, O0, and debug on all 15 builds.
Compilation improves **75 → 150 / 150 objects**. Exact function counts remain
**350 / 1,350**; the newly compiled rebinding functions are not exact yet.
The **10,800 native comparisons** have no unexpected failures. They separately
record 16 remaining candidate Wii O0 volatile-read differences and 16 reference
GC/1.1p1 O0 cases where the second parameter overwrites saved r30 at 8(sp).
The latter reference ABI bug remains unimplemented. The baseline has **1,184**
volatile-read mismatches on the previously compilable sources.

In full BfBB source, `__AXVPBInit` now inlines `__AXSetPBDefault` and shares its
member values within the loop. All **240 comparisons** pass across baseline,
candidate, fresh references, the original GQPE78 executable, and the memory
model. Inlining increases the initializer from **512 → 564 bytes** on the
oldest four builds and **520 → 572** on the other eleven. Reference sizes are
512/520/548, so this is composition progress, not a size or exactness win:
`__AXVPBInit` remains **0/15 exact**, while the standalone default helper stays
**15/15 exact**. Frame policy and value reuse across the caller/helper boundary
remain matching work. `AXSetVoiceSrcRatio` changes only an anonymous float
symbol's ordinal (`@194` to `@197`), retaining identical instructions and value.

Removing redundant inline pointer snapshots also shrinks the GC/1.2.5n full
`__AXSyncPBs` **780 → 764 bytes**, `AXAcquireVoice` **628 → 624**, and
`GXSetVtxAttrFmtv` **848 → 812**. Original-executable comparisons pass for all
**256** full sync cases, **10,240** AX allocation cases across ten functions,
and **3,072** GX cases across three functions. All ten AX and fourteen GX
library sources compile; eight AX objects and thirteen GX objects are unchanged.
These targeted results do not establish whole-library exactness.

Regression checks: **1,065 / 1,110** older objects are identical; the other 45
are the improved guarded member initializers. All **2,626** compiled recent
objects are identical, with the same 89 compilation failures. The indexed
panel preserves all **1,674** statuses and object comparisons, including 1,114
compiled and 972 previously exact cases. **420** preceding BSS, fill-loop,
split-address, and array-cursor objects are byte-identical to their validated
predecessors. Backend tests: **1,652 passed**, with the existing nested-assembly
inline test excluded. The full corpus was not rerun.

Local evidence: `target/member-inline-final-verification.json` binds the final
compiler fingerprint, 602 execution-tested object hashes, and 420 preserved
objects. Measurements and native results are under `target/member-inline-*`;
`target/verify_member_inline_final.py` checks those bindings. Supplemental full
AXVPB reference flags continue to remove `-W err` on every side.

## Global-array pointer induction and loop-entry scheduling, 2026-09-08

BfBB `__AXVPBInit` now carries its four array pointers between iterations,
replacing repeated global-address materialization and index scaling with
constant pointer increments. The oldest four builds shrink **540 → 512 bytes**;
the other eleven shrink **536 → 520**. Reference sizes are 512 for the oldest
four, 520 for the middle eight, and 548 for GC 3/Wii. Matching sizes are not
matching output: the initializer remains **0/15 exact**. Only this function
changes in each full-source object, and `__AXSetPBDefault` retains **15/15 exact**.

All **240 native comparisons** pass across baseline, candidate, fresh references,
the original GQPE78 executable, and the full-memory model. With clock input zero
and the same callback stubs, instruction counts fall **21,744 → 20,981** on the
oldest builds and **21,743 → 20,983** on the other eleven: **760–763 fewer executed
instructions**. These are instruction counts, not timing estimates. Reference
counts are 20,713 / 20,729 / 20,689. The supplemental full-source flags still
remove `-W err` on every side.

The source-tree owner recognizes leading `pointer = &global[index]` bindings
in a top-level, zero-initialized, constant-bound, unit-step `for` loop at O2+.
It reuses the original pointer locals, moving their initial addresses into the
loop initializer and appending ordinary typed increments to the step. The
normal allocator owns their cross-call lifetimes. The proof requires private,
nonvolatile automatic bindings, equal array/pointer strides, no local shadowing
of the global array, no index or pointer rebinding in the body, and no pointer
observation after the loop. Escaped locals, alternate entries, opaque assembly,
nested loops, early exits, and unrecognized control flow decline the rewrite.
Assignments through a pointer remain memory writes, not pointer rebindings.
O0 retains its source loop; schedule-off still permits induction.

Reduced probes also exposed an existing linkage-first entry-scheduling bug:
a backward branch after the first call can enter argument setup before that
call. Hoisting a ready constant into the prologue both skips its per-iteration
materialization and leaves intervening numeric branch targets attached to the
wrong instruction. Entry argument and wide-mask scheduling now check incoming
targets. Zero-store scheduling keeps its measured call-free invariant case only
when backedges preserve r0; callbacks and r0 redefinitions require the original
per-iteration zero.

Canaries **2099–2103** add ten forms at O4, O2, schedule-off, O0, and debug:
one/two array cursors, chained stores, conditional stores, a returned last
element, pointer mutation, scalar arrays, zero stores, a nonunit index, and an
explicitly written moving pointer. All three sides compile **75/75 objects**.
All **12,000 candidate/reference native cases** pass. The baseline has **320
failures** in the explicit pointer loop on the oldest four builds, across all
five flag sets; the candidate fixes them. Checks cover callback arguments and
order, memory at every callback and return, randomized surrounding storage,
returned pointers, volatile-register clobbers, saved GPRs/FPRs, SP, LR, and PC.
There are no new reference-bug exceptions. Exact function text plus symbolic
relocations remains **0/750**, and whole objects **0/75**; 380 functions change,
with no lost exact functions in this panel.

Focused regressions retain **1,110 identical objects** from 2002–2075,
**2,626 identical objects / 89 unchanged failures** from 1821–2001, and
**1,114 identical compiled objects / 972 known exact matches** in the indexed
panel. All **345 preceding fill, BSS-boundary, and split-pointer objects** retain
their candidate hashes and prior execution evidence. The ten AX and fourteen
GX units compile under strict GC/1.2.5n project flags; only AXVPB changes.
Backend tests pass **1,647 cases**, including four new proof/scheduling cases,
with the existing embedded-assembly exclusion unchanged.

The final manifest binds **270 execution-tested object hashes** and the 345
preserved objects to the compiler/harness fingerprint above. Artifacts are
`target/array-cursor-{canaries,versions,older,recent,index,preserved,ax-library,library}`
and `target/array-cursor-final-verification.json`. Remaining initializer work
includes register/frame placement, default-helper inlining, setup/store/tail
scheduling, and GC 3/Wii's separate byte-offset induction form for the full
four-array transaction.

## Shared pointer halves and indexed field stores, 2026-09-08

The complete BfBB `__AXVPBInit` now shares repeated high/low pointer values,
removing **thirteen instructions / 52 bytes on every reference build**.
The oldest four builds shrink **592 → 540 bytes**, and the other eleven
**588 → 536**, versus 512/520/548-byte references. Only this function changes
in each full-source object; `__AXSetPBDefault` retains **15/15 exact matches**.
All **240 native cases** still agree across baseline, candidate, fresh
references, the original GQPE78 executable, and the independent full-memory
model. The initializer remains **0/15 exact**: frame layout, pointer induction,
inlining, and remaining setup/store scheduling are still outstanding. The
supplemental complete-source panel keeps `-W err` removed on every side.

A pre-allocation value graph follows register copies, constant pointer offsets,
and logical high-half extraction across halfword stores. It shares the computed
address and high half and uses the original pointer directly for a zero-offset
low half. Memory aliases do not invalidate register-derived values; all store
targets remain intact. Branch entries split regions. Symbolic fixups, mutated
input/address registers, escaping scratch values, return-home definitions,
opaque assembly, jump tables, and additional entry points exclude the owner.
New values use virtual registers and ordinary CFG liveness/allocation.

O2 and schedule-off materialize values at first use. O3/O4 issue the first
store, then the other independent address values, then their remaining stores.
`split_address_low_store_first` independently selects GC 3/Wii's ordinary leaf
exchange: a ready low half can precede its matching high-half store when both
write disjoint fields of the same positively nonvolatile pointer. The exchange
requires a complete frameless leaf run; volatile stores and larger function
fragments preserve source order. O0 keeps ordinary scalar lowering.

The old indexed-pointer-store scheduling rejection now admits runs whose
halfword targets are parameter-based constant indices/dereferences and whose
values are parameter casts, constant offsets, or high halves. These use existing
scalar lowering and the same value graph; other rejected families retain their
existing diagnostics.

Canaries **2089–2098** add eleven forms across member/indexed fields, O4, O3,
O2, schedule-off, O0, and debug: shared halves, positive/negative offsets, three
addresses, low-first source order, volatile stores, overwritten fields, returned
pointers, callbacks, guards, and pointer mutation. Candidate and reference
compile **150/150 objects**; the baseline compiles **90 member objects** and
rejects all **60 indexed objects**. On the member subset, exact function text
plus symbolic relocations improves **0 → 463/990**. The newly compiling indexed
subset contributes **281/660 exact functions**, for **744/1,650** overall.
No existing exact functions are lost; whole objects remain **0/150**.

All **211,200 candidate/reference native cases** pass, and the baseline's
**126,720 executable member cases** also pass. Checks cover equal and partially
overlapping output pointers, randomized surrounding memory, high/low arithmetic
boundaries and wraparound, callback memory effects and volatile-register
clobbers, returned pointers, saved registers, SP, LR, and PC. Volatile cases
also check every store's address, width, value, and order.

Focused regressions retain **1,110 identical objects** from 2002–2075,
**1,114 identical compiled objects / 972 known exact matches** in the indexed
panel, and **2,626 identical objects / 89 unchanged failures** from 1821–2001.
The preceding fill and BSS-boundary panels retain all **195 candidate object
hashes**, preserving their prior execution evidence and exact counts. All ten
AX and fourteen GX units compile under GC/1.2.5n project flags; only AXVPB
changes. Tests pass **1,643 backend** cases, including five new value-graph
proof cases, and **55 version** cases, with the existing embedded-assembly
backend test excluded.

Artifacts are under `target/split-address-{canaries,versions,older,index,recent,library,ax-library}`.
`target/split-address-final-verification.json` recompiles candidate panels,
binds **435 execution-tested object hashes**, and records the 195 preserved
preceding objects against the final compiler and harness. Complete-project
compilation, linking, and matching remain unfinished.

## Constant pointer-fill loops and prologue backedges, 2026-09-08

The three clearing loops in the complete BfBB `__AXVPBInit` now use the
reference's **eight-store CTR bodies on all fifteen builds**. Their steady
state changes from six instructions per word to ten instructions per eight
words. All **240 native cases** still agree with baseline, fresh references,
the original GQPE78 executable, and the independent full-memory model. Only
`__AXVPBInit` changes in each complete-source object; `__AXSetPBDefault` retains
**15/15 exact matches**. The initializer is still **0/15 exact**: sizes grow
**520 → 592 bytes** on the oldest four builds and **516 → 588** on the other
eleven, versus 512/520/548-byte references. Unrolling adds the expected stores;
frame selection, setup scheduling, inlining, pointer induction, and repeated
split-pointer expressions remain outstanding. The supplemental version panel
continues to remove `-W err` on every side.

A shared pre-allocation pass proves a single-entry, constant-trip countdown
loop with one constant byte/halfword/word store and one pointer advance.
It preserves observable counter and pointer exit values through CFG liveness,
requires the removed comparison's CR0 result to be dead, and rejects extra
entries, instruction-owned metadata in the body, opaque assembly, and existing
CTR ownership. A fresh virtual register supplies CTR; branch/label and metadata
remapping use the common instruction-editing helpers. Store order is preserved.
O0–O2 remain outside the expansion owner.

`FixedFillLoopStyle` separates version policy from recognition. Through GC/2.7,
performance optimization selects the largest exact divisor up to ten stores.
GC 3/Wii fully expand counts below 64; larger fills divide complete eight-store
packets into batches of up to 56 stores and emit up to seven remainder stores.
For example, 100 becomes two 48-store iterations plus four final stores. Size
optimization uses a one-store CTR loop. GC 3/Wii switch the **entire function**
back to the ten-store divisor policy when it references an explicitly aligned
global. Probes include alignment 4, unrelated parameter fills, separate loop
pointers, and unreferenced aligned declarations; this explains the full AX
initializer's smaller batches.

Canaries **2082–2088** add 27 forms across O4, O3, O2, size, schedule-off, debug,
and O0: zero/small/large counts, prime counts and packet remainders, three store
widths, returned counter/pointer values, conditional and nested loops, callbacks,
repeated fills, volatile writes, and aligned-global context. All sides compile
**105/105 objects**. Exact function text plus symbolic relocations improves
**3 → 633/2,835**, with no lost matches; whole objects remain **0/105**.
All **181,440 native cases** pass on the candidate and have no unexpected
reference failures. Checks cover randomized surrounding memory, every write's
address/width/value and order, callback effects and volatile-register clobbers,
return values, preserved registers, SP, LR, and PC.

The execution panel also exposed **5,376 baseline failures** on the oldest four
builds: the plain linkage-first scheduler hoisted a loop-entry constant above
`stwu`, causing each backedge to allocate another frame. Prologue latency work
now stops at branch entries, fixing the O0/O2 paths as well as optimized fills.
A separate **64-case GC/1.1p1 O0 saved-r30 spill alias** is modeled as an existing,
unimplemented reference bug: `conditional` spills `yes` over saved r30 at
`8(sp)`. These cases are not claimed as ABI parity.

Previous boundary canaries **2076–2081** retain **118/720 exact functions**, no
lost matches, and **92,160 passing native cases** on all sides, including their
separately modeled 128-case reference spill bug. Focused regressions retain
**1,110 identical objects** from 2002–2075, **1,114 identical compiled objects /
972 known exact matches** in the indexed panel, and **2,626 identical objects /
89 unchanged failures** from 1821–2001. All ten AX and fourteen GX units compile
under GC/1.2.5n project flags; only AXVPB changes. Tests pass **1,638 backend**
(including four new loop-proof cases) and **55 version** cases, with the existing
embedded-assembly backend test excluded.

Artifacts are under `target/fill-unroll-{canaries,previous,versions,older,index,recent,library,ax-library}`.
`target/fill-unroll-final-verification.json` recompiles candidate panels and
binds **630 execution-tested object hashes** to the final compiler and harness.
These are targeted progress measurements; complete project compilation,
linking, and matching remain unfinished.

## Complete BSS addresses and the AX voice initializer, 2026-09-08

The complete BfBB `AXVPB.c` initializer now passes **240 native execution
cases across all fifteen builds**, against fresh references, the original
GQPE78 executable, and an independent memory model. The baseline fails all
**64 cases on GC/1.1, GC/1.1p1, GC/1.2.5, and GC/1.2.5n**: the address of
`__AXVPB` at BSS offset `0x8d00` loses its high half, so its clearing loop
writes 64 KiB before the intended array. The fix adds the adjusted high half
before the low displacement. Checks cover randomized memory around all four
arrays, all 64 initialized voices and parameter blocks, callback arguments
and order, bus-clock division, counters, saved registers, SP, LR, and PC.

Only `__AXVPBInit` changes in the four affected complete-source objects,
from **516 to 520 bytes**, versus the 512-byte references. The other eleven
objects are unchanged. The initializer still has **0/15 exact function
matches**; `__AXSetPBDefault` retains **15/15 exact matches**. The supplemental
full-source version panel removes `-W err` on every side, as before.

BSS ordering and section routing now have one shared implementation used by
the object writer and a machine-code finalization pass. A distinct
`SymbolAddress` displacement marks a complete address; existing low-half-only
fixups retain their meaning, including explicitly biased section pages.
Finalization runs after unit layout is available and before debug lowering.
It inserts `addis` when necessary, preserving the section anchor and using
physical CFG liveness if an r0 result needs a temporary. Branches, entry points,
and jump-table destinations target the beginning of the expansion; relocation
and displacement owners follow their original instructions. Consumed addresses
keep their low fixup and symbol-discovery event. Initialized-data address
expansion remains a separate follow-up.

Canaries **2076–2081** cover signed-low/page boundaries through `0x18000`,
conditional and repeated calls, returned/stored pointers, and function-local
static declaration ordering, at O4, O2, schedule-off, O0, C++, and debug.
All three sides compile **90/90 objects**. All **92,160 native cases** pass
on the candidate, eliminating **21,504 baseline failures**. The references
have no unexpected failures. A separately modeled **128-case GC/1.1p1 O0
saved-r30 spill alias** remains an unimplemented reference bug: `repeated`
spills its count over the saved register at `8(sp)`. These cases are not
claimed as ABI parity. Exact function text plus symbolic relocations remains
**118/720**, with no lost matches; whole objects remain **0/90**.

Focused regressions retain **1,110 identical objects** from 2002–2075,
**1,114 identical compiled objects / 972 known exact matches** in the indexed
panel, and **2,626 identical objects / 89 unchanged failures** from 1821–2001.
All **ten AX and fourteen GX units** compile with the GC/1.2.5n project flags;
only the expected AXVPB object changes. Tests pass **three address-finalizer**,
**40 object-writer**, and **1,634 backend** cases, with the existing embedded-asm
backend test excluded.

Artifacts are under `target/section-address-{canaries,versions,older,index,recent,library,ax-library}`.
`target/section-address-final-verification.json` recompiles the candidate panels
and binds **315 execution-tested object hashes** to the final compiler and
harness. The original executable SHA-256 is
`865f446cf8efd52230dac506d6334a357b4c738cd6eb6e3a68c827efac51c3e0`;
its symbol-file SHA-256 is
`92f658ac1a6f91ec64d851dcfdf92a0907f3c06f5c68a97096052e99b5f3dfa4`.
This is a correctness milestone; complete project builds and matching remain
unfinished.

## Member-value scheduling across all fifteen builds, 2026-09-08

The complete BfBB `AXVPB.c` now produces **exact 64-byte `__AXSetPBDefault`
functions for all fifteen reference compiler builds**, improving **3 → 15/15**.
GC/1.1p1 gets its early interior-pointer calculation; the eleven later builds
shrink from 80 to 64 bytes. The other three builds retain their exact output,
including all sixteen instruction words in the original GQPE78 executable.
Only `__AXSetPBDefault` changes in the twelve affected full-source objects.
All **30,720 native cases** pass against baseline, fresh references, the original
executable, and the independent field model. The complete-source version panel
retains the supplemental project flags with `-W err` removed on every side.

`MemberValueSchedule` now independently selects the leaf initialization
schedule. Ordinary 2.3.3 commits the first store before the remaining values;
the patched build advances the interior pointer; middle-generation builds issue
two values before the first store. GC 3/Wii prioritize shared values, then the
interior address, then single-use literals. Ordered pointers retain their
leading source value and the build's one- or two-value issue window. Existing
positive nonvolatile-pointer facts choose the ordinary ready-value path.
Every policy retains source store order and uses the same value graph, fresh
virtual registers, reverse consumer allocation group, and ordinary liveness.
With scheduling disabled, each shared value materializes at its first store;
O0 remains outside this owner. The previous shape and metadata exclusions remain.
An enumerated graph test verifies each value is defined exactly once before
its stores and that every schedule preserves all source stores in order.

Canaries **2072–2075** add fourteen forms at O4, O2, schedule-off, and O0:
leading/middle/trailing interior pointers and marker values; two shared values;
repeated markers; a narrow all-ones literal; overwritten fields; three volatile
orders; and a returned receiver. All sides compile **60/60 objects**. Exact
function text plus symbolic relocations improves **239 → 672/840**, with whole
objects still **0/60**. All **215,040 native cases** pass on baseline, candidate,
and reference, checking full randomized memory regions, volatile write order,
returned pointers, preserved registers, SP, LR, and PC.

The preceding canaries **2068–2071** improve **328 → 521/765 exact functions**,
with no lost exact matches and **97,920 native cases** passing on all three
sides. Remaining examples include narrow literal materialization, non-void
receiver returns, and GC 3's ordinary-memory store reordering and dead-store
removal. Those differences are recorded without changing source memory order
in the new scheduler.

Focused regressions retain **990/990 identical objects** from 2002–2067,
**1,114 identical compiled objects / 972 known exact matches** in the indexed
panel, and **2,626 identical objects / 89 unchanged failures** from 1821–2001.
All **ten AX and fourteen GX objects** are unchanged under the GC/1.2.5n project
flags. Tests pass **1,634 backend** and **54 version** cases, with the existing
embedded-assembly backend test excluded.

Artifacts are under `target/member-schedule-{canaries,previous,versions,older,index,recent,library,ax-library}`.
`target/member-schedule-final-verification.json` binds **405 execution-tested
object hashes** and the focused regression checks to the final compiler and
harness. These are targeted function results; complete project compilation,
linking, and matching across versions remain unfinished.

## Member initialization values and array displacements, 2026-09-08

The complete BfBB `AXVPB.c` now produces an exact **64-byte
`__AXSetPBDefault`** for GC/1.1, GC/1.2.5, and GC/1.2.5n, improving from
**92 bytes** and **0 → 3/15 exact version outputs**. GC/1.2.5n also matches
all **16 original-DOL instruction words** with no unresolved relocations.
The pinned layout is `docs/reference-layouts/bfbb-axvpb-default.json`.
GC/1.1p1 reaches 64 bytes but still differs in scheduling: its reference
materializes the interior pointer before the first store. The eleven later
builds shrink to 80 bytes and remain unmatched against their 64-byte references.
All **30,720 native cases** agree across baseline, candidate, fresh references,
and the original executable. Checks include every byte of the randomized
1,024-byte object region and saved registers, SP, LR, and return PC.

A new pre-allocation member-store owner retains repeated immediate values and
an interior pointer across a complete void leaf initialization. It recognizes
one common base, word/halfword/byte stores, and two or three distinct values,
including exactly one interior pointer. Source store order is preserved,
including assignment chains and volatile writes. The old constant-store
schedule issues the first store immediately and then the remaining value
materializations. Reverse consumer allocation order and scratch preferences
produce the observed overlapping homes through ordinary liveness, without
prescribing physical r4/r5. The owner requires O2 or higher, an enabled scheduler,
and the InterleavedPairs version policy. Calls, branches, loads, frames, other
bases, non-void returns, and instruction-indexed output metadata exclude it.
The patched and newer schedules remain follow-up work.

Literal member-array indices now use ordinary displacement stores when the
combined byte offset fits signed 16 bits. Larger offsets retain the indexed
address path. This also removes three instructions from the inlined zero-store
chain in `__AXSyncPBs`; its other instructions and relocation targets are
unchanged after accounting for shifted branch targets. Complete execution or
matching of `__AXSyncPBs` is not claimed here.

The corpus exposed a separate pointer-arithmetic bug: a nonzero-offset inline
halfword array's `p->data + 2` advanced two bytes instead of four. Add/subtract
and commuted literal additions now fold the member offset together with the
scaled element offset when the result fits an address displacement. Explicit
array strides are respected. This fixes **30,720 baseline execution failures**
in the reduced pointer and initialization forms.

Canaries **2068–2071** cover twelve forms at O4, O2, explicit schedule-off, and
O0 across fifteen builds; the three optimized files also cover stores at byte
offsets 32,766, 32,768, and 33,998. All sides compile **60/60 objects**. Exact
function text plus symbolic relocations improves **82 → 328/765**, while whole
objects remain **0/60**. All **97,920 candidate and reference native cases**
pass the independent model, including volatile write order, parameter-base
placement, neighboring bytes, large-offset writes, and ABI state. The large
literal-store form remains unsupported at O0 in both baseline and candidate,
so it is confined to the three optimized canaries.

The focused regression panels retain **990/990 identical objects** from
2002–2067, **1,114 identical compiled objects / 972 known exact matches** in
the indexed panel, and **2,626 identical objects / 89 unchanged failures**
from 1821–2001. All fourteen GX objects and nine of ten AX objects remain
identical; all ten AX units compile. Only `__AXSyncPBs` and `__AXSetPBDefault`
change within AXVPB. The fifteen-build full-source panel uses the existing
supplemental flags with `-W err` removed on all sides; the GC/1.2.5n AX/GX
library check retains the project flags. Backend tests pass **1,633**, with
the single known embedded-assembly test excluded.

Artifacts are under `target/member-init-{canaries,versions,index,recent,previous,library,ax-library}`.
`target/member-init-final-verification.json` binds **225 execution-tested
object hashes** and the focused regression checks to the final compiler and
harness. Full-project compilation, linking, and cross-version matching remain
unfinished; these counts describe targeted samples, not corpus-wide parity.

## Patched address scheduling and live constant-store homes, 2026-09-08

GC/1.1p1 now matches **all five AXSPB functions**, including the complete
**1,016-byte `__AXPrintStudio`**. The fifteen-build comparison improves
**63 → 64 of 75 exact functions**: GC/1.1, GC/1.1p1, GC/1.2.5, and GC/1.2.5n
match **5/5**, while the eleven later builds remain **4/5**. The complete-source
panel retains the preceding checkpoint's supplemental flags (`-W err` removed
on all sides) and normalizes the reference BSS anchor to `__AXStudio`.
All **76,800 native cases** agree across baseline, candidate, fresh references,
and the original executable. GC/1.2.5n's exact original-DOL output is unchanged.

A version policy isolates the patched compiler's address completion schedule.
The address high half survives quotient rounding; the low half follows the
completed quotient. Separate virtual values and a joint consumer allocation
group reproduce the register homes without prescribing physical registers.
An optional copy-source preference lets the branch-local quotient copy reuse
its source's allocated home. It is subordinate to explicit physical preferences
and all ordinary interference, exclusion, pool, and call-survival checks.
Incoming branches, dependencies on the unfinished address, unsupported division
shapes, and unrelated relocations prevent deferral. The policy requires an
enabled scheduler and O2 or higher.

The new corpus also exposed two clobbers in the old physical constant-store
scheduler. It now excludes incoming homes live through the surrounding CFG,
retains the original sequence when its final scratch value is still needed,
and declines coloring when no legal register is available. This fixes callback
addresses overwritten by branch-local constants: **1,872 baseline execution
failures become zero**. In the complete BfBB `AXVPB.c`, the same scratch-lifetime
check fixes `__AXSetPBDefault`, which previously wrote **164 instead of zero**
to `updateMS`. All **2,048 candidate executions** match fresh GC/1.2.5n, the
original executable, and the independent field model; all baseline executions
fail that field. The function grows **84 → 92 bytes** and still differs from
the **64-byte reference**. Other AXVPB functions are unchanged.

Canaries **2064–2067** cover eight forms across fifteen builds at O4, O2,
explicit schedule-off, and O0: branch-local, linear, and mixed member stores;
loops; values surviving callbacks; inline stores; retained-dividend division;
and nested branches. All sides compile **60/60 objects**. Whole-object and
complete function text plus symbolic relocation matching remain **0/60** and
**0/480**. All **122,880 candidate native cases** match the ordinary model,
including callback addresses and effects, neighboring memory, and ABI state.

GC/1.1p1 O0 `looped` exposes another **unimplemented reference spill bug**:
the count parameter overwrites saved r30 at `8(r1)`, so the epilogue restores
the count into the caller's r30. All **256 affected reference cases** match
that separate corruption model; baseline and candidate preserve the caller's
r30 instead. The remaining **122,624 reference cases** match the ordinary
model. These 256 cases are a parity gap, not successful behavior matches.
Later compilers' branch-local address rematerialization also remains open:
inline pointer bindings currently form addresses before their branch consumers,
so changing only the global-base cache policy does not reproduce those outputs.

The two changed older objects (GC/1.1p1 canaries 2060 and 2061) pass **4,608
paired native cases**. The other **928/930 objects** in the 2002–2063 panel
remain identical. The indexed panel retains **1,114 identical compiled objects**
and **972 known exact matches**; canaries 1821–2001 retain **2,626 identical
objects** and **89 unchanged failures**. All fourteen configured GX objects and
nine of ten AX objects are identical; all ten AX units compile, with only
AXVPB changing. Tests pass **1,631 backend**, **113 allocator**, and **53 version**
tests, with the known embedded-assembly failure excluded and eight existing
allocator tests ignored.

Artifacts are under `target/branch-base-{canaries,old-entry,versions,index,recent,previous,library,ax-library}`.
`target/branch-base-final-verification.json` pins the final compiler/harness and
verifies **234 execution-tested object hashes** after the final rebuild.
Full-project compilation, linking, and matching across versions remain
unfinished; these focused counts are not a corpus parity estimate.

## Complete AXSPB function matching, 2026-09-08

All **five functions** compiled from the complete BfBB `AXSPB.c` now match the
pinned original DOL exactly after relocation. `__AXPrintStudio` improves
**249 → 254 of 254 linked instruction words**, remaining **1,016 bytes**.
Fresh GC/1.2.5n also matches **5/5**, with zero unresolved relocations on either
side. The project flags are unchanged. All **5,120 three-way native comparisons**
pass against the fresh reference, original executable, and independent model,
including cache-flush callbacks, all studio and input bytes, and saved-register,
stack, and return-state checks. This is complete function matching for this
translation unit, not full-project linking or matching.

The pre-allocation constant-division scheduler can now form a complete
reciprocal constant before an independent retained object address and dividend
load. Both address halves must refer to the same symbol; register dependencies,
internal branch entries, relocated constants, volatile inputs, and opaque
control flow prevent the move. Branch entries execute the complete reordered
packet while relocations and other metadata follow their instruction owners.
The division emitter supplies consumer-first allocation groups for its result,
rounding temporaries, and retained dividend. The ordinary allocator handles
interference and register reuse. Both changes require an enabled scheduler and
O2 or higher; no function names or physical result registers are prescribed.

A supplemental complete-source comparison covers **fifteen compiler builds**.
Exact function text plus symbolic relocations improve **60 → 63 of 75**:
GC/1.1, GC/1.2.5, and GC/1.2.5n now match **5/5**; the other twelve builds remain
**4/5**, with `__AXPrintStudio` still different. The reference's BSS anchor is
normalized to `__AXStudio` for this comparison. All **76,800 native cases**
agree across baseline, candidate, fresh reference, and the original executable.
The strict project flags compile **15/15 candidate and baseline objects** but
only **12/15 references**: GC/3.0a3, GC/3.0a3p1, and Wii/1.0 promote an existing
`vi.h` parameter-scope type warning to an error. The supplemental fifteen-build
comparison removes `-W err` on all sides; all **15/15** then compile. This flag
adjustment does not establish strict diagnostic parity for the newer builds.

Canaries **2060–2063** cover nine forms at O4, O2, explicit schedule-off, and O0:
signed divisors 3, 7, and 160; unsigned division; volatile input; preceding stores
and callbacks; a second-position terminal address argument; and two inline fade
updates. All sides compile **60/60 objects** and pass **138,240 paired native
cases** with arithmetic boundaries, changing volatile reads, callback arguments,
callback memory effects and clobbers, neighboring bytes, and ABI checks. The
first reciprocal-constant/address order matches **30 → 180 of 270 optimized
function samples**. Whole-object and complete function matching remain **0/60**
and **0/540** respectively; other address placement and code-generation gaps
remain. All thirty schedule-off/O0 objects are identical to baseline.

All **870 objects** in the 2002–2059 panel remain identical. The indexed panel
retains **1,114 identical compiled objects** and **972 known exact matches**;
canaries 1821–2001 retain **2,626 identical objects** and **89 unchanged failures**.
All fourteen configured GX objects and the other nine AX objects remain
identical; all ten AX units compile. Backend tests pass **1,626**, with the
previously confirmed embedded-assembly failure excluded.

Artifacts are under `target/entry-magic-{canaries,real,versions,index,recent,previous,library,ax-library}`.
The versions directory retains both strict and supplemental compilation results.
`target/entry-magic-final-verification.json` pins the final compiler/harness and
**227 execution-tested object hashes**, including the full fifteen-build panel.
Full-project compilation, linking, and matching across versions remain
unfinished; these focused counts are not a corpus parity estimate.

## Consumer-first allocation, 2026-09-08

The complete BfBB `__AXPrintStudio` now matches **249/254 linked instruction
words at their reference offsets**, up from **213/254**, at **1,016 bytes**.
All nine fade bodies match; five entry instruction words still differ. AXSPB
retains **4/5 exact candidate functions**, **5/5 exact fresh reference functions**,
zero unresolved relocations, and **5,120 passing three-way native comparisons**
against fresh GC/1.2.5n, the original DOL, and the independent model.

A pluggable consumer-first allocation policy visits each nominated expression's
result before its inputs. Global updates now expose distinct multiply, reload,
and subtraction values instead of forcing them into a single temporary home.
The policy shares LinearScan's register selection and preserves CFG interference,
pinned homes, preferences, exclusions, and call survival. Expiration accounts
for every remaining definition when visits run out of source order. Physical
multiply temporaries migrate only when CFG liveness proves no later consumer.
See [the allocator notes](register-allocator.md) for the policy boundary.

Canaries **2056–2059** cover six forms across fifteen builds at O4, O2, explicit
schedule-off, and O0: retained arguments, a later delta use, byte stores,
consecutive updates, a callback frame, and a preceding volume store. All sides
compile **60/60 objects**. Exact function text plus symbolic relocation matches
increase **140 → 162 of 360**; whole-object matching remains **12/60**. Native
checks exercise **92,160 paired cases** with arithmetic boundaries, aliased
outputs, callback effects and clobbers, neighboring bytes, and saved-register,
stack, and return-state checks.

Those executions expose an **unimplemented reference bug** in GC/1.1p1 O0
`framed` (canary 2059): MWCC spills the output pointer and callback argument to
`8(r1)`, then writes the negated halfword through the callback argument. All
**256 affected cases** reproduce that corrupted destination using a mapped
callback argument and a separate bug model. Baseline and candidate instead
match the C model; this is a parity gap, not a passing behavior match. The
remaining **91,904 cases** agree with the ordinary model on all sides, with no
unexpected mismatches.

The changed O4/O2 objects from canaries 2052–2053 improve **64 → 212 of 390**
exact functions and pass **99,840 paired native comparisons**. The fifteen
changed objects from canary 2022 pass another **15,360**. The other **765/810
objects** in the 2002–2055 panel remain identical. The indexed panel retains
**1,114 identical compiled objects** and **972 known exact matches**; canaries
1821–2001 retain **2,626 identical objects** and **89 unchanged failures**.
All fourteen configured GX objects and the other nine AX objects remain
identical; all ten AX units compile. Backend tests pass **1,623**, with the
previously confirmed embedded-assembly failure excluded. Allocator tests pass
**110**, with eight existing ignored tests.

Artifacts are under `target/consumer-order-{canaries,old-negates,old-inline,real,index,recent,previous,library,ax-library}`.
`target/consumer-order-final-verification.json` pins the compiler/harness and
verifies **302 execution-tested object hashes** after the final rebuild.
Full-project compilation, linking, and matching across versions remain
unfinished; these focused counts are not a corpus parity estimate.

## Negated delta scheduling, 2026-09-08

The complete BfBB `__AXPrintStudio` now computes all nine negated deltas before
their host-update multiplies, matching that part of the reference schedule.
It matches **213/254 linked instruction words at their reference offsets**, up
from **204/254**, and remains **1,016 bytes**. The allocation of the multiply
result and host reload still differs, as does the entry schedule. AXSPB retains
**4/5 exact candidate functions**, **5/5 exact fresh reference functions**, no
unresolved relocations, and **5,120 passing three-way native comparisons**
against fresh GC/1.2.5n, the original DOL, and the independent model.

A separate scheduling pass recovers the independent update value hidden by
r0 reuse. The global update receives a virtual temporary so the negated value
can occupy r0 earlier while memory operations retain their source order.
Version profiles put a pending negation after the subtraction in early
GameCube builds and before it in later builds. A preceding word store allows
the negation to lead the multiply/load sequence. The pass requires an enabled
scheduler and O2 or higher, an unchanged delta, a matching direct-SDA word
load/store pair, and a terminal narrow store. Incoming control-flow edges,
relocations outside the memory pair, and opaque flow prevent an unsafe move;
branch entries and instruction metadata are remapped with the schedule.

Canaries **2052–2055** cover thirteen forms across fifteen builds at O4, O2,
explicit schedule-off, and O0: byte/halfword deltas, positive and negative
multipliers, unsigned hosts, live and changed deltas, volatile host updates,
calls, computed locals, clamping, and preceding stores. All sides compile
**60/60 objects**. Exact function text plus symbolic relocation comparisons
increase **242 → 298 of 780**; whole-object matching remains **0/60**. All
**199,680 paired native comparisons** pass, including aliases between volume,
host, narrow output, and the final live-value store; signed wrap boundaries;
callback effects and clobbers; volatile reads; neighboring bytes; saved GPR/FPR
images; and SP/LR/PC. Baseline and references also have no execution failures.

The fifteen changed older objects from canary 2022 pass another **15,360 paired
native comparisons**. The other **735/750 objects** in the 2002–2051 panel
remain identical. The indexed panel retains **1,114 identical compiled objects**
and **972 known exact matches**, and canaries 1821–2001 retain **2,626 identical
objects** and **89 unchanged failures**. All fourteen configured GX objects and
the other nine AX objects remain identical; all ten AX units compile. Backend
tests pass **1,622**, with the previously confirmed embedded-assembly failure
excluded; all **52 version tests** pass.

Artifacts are under `target/early-negate-{canaries,old-inline,real,index,recent,previous,library,ax-library}`.
`target/early-negate-final-verification.json` pins the compiler/harness and
verifies **212 execution-tested object hashes** after the final rebuild.
Full-project compilation, linking, and matching across versions remain
unfinished; these focused counts are not a corpus parity estimate.

## Constant-division load scheduling, 2026-09-08

The complete BfBB `__AXPrintStudio` now matches **204/254 linked instruction
words at their reference offsets**, up from **107/254** in the preceding
compiler. Its size remains **1,016 bytes**. Starting the division constant's
high half before the dividend load also gives the allocator MWCC's definition
order, fixing the dividend and quotient homes through the clamp branches.
This is still **not an exact function match**: entry scheduling, negation
placement, and host-subtraction registers remain different. AXSPB retains
**4/5 exact candidate functions**, **5/5 exact fresh reference functions**, zero
unresolved relocations, and **5,120 passing three-way native comparisons**
against fresh GC/1.2.5n, the original DOL, and the independent model.

A separate instruction-scheduling module recognizes a direct nonvolatile SDA
word load followed by a constant high/low pair and the consuming signed or
unsigned high-word multiply. It overlaps the independent high half with the
load before register allocation, including in structured bodies that already
own their schedules. The transformation requires O2 or higher and an enabled
scheduler. Register dependencies, relocated constants, and internal branch
entries prevent the swap. An entry at the original load is redirected to the
whole reordered pair, while relocations and other instruction metadata follow
their actual instruction owners. Volatile globals and opaque control flow are
excluded.

Canaries **2048–2051** cover signed/unsigned divisors 3, 7, and 160, changing
volatile inputs, and a join immediately before division, at O4, O2, explicit
schedule-off, and O0. Baseline, candidate, and references compile **60/60
objects** across fifteen builds. The reciprocal-constant/load operation order
matches in all **315 optimized nonvolatile function samples**, including the
schedule-off controls. Exact output remains **0/60 whole objects** and
**0/480 function text plus symbolic relocation comparisons**. All **122,880
paired native comparisons** pass independent arithmetic, aliasing, neighboring
memory, volatile-read, and ABI checks; baseline and references also have no
execution failures.

The ninety changed older objects from canaries 2022, 2029, 2032, 2033, 2044,
and 2047 pass another **207,360 paired native comparisons**. The other
**600/690 objects** in the 2002–2047 panel remain identical. The indexed panel
retains **1,114 identical compiled objects** and **972 known exact matches**;
canaries 1821–2001 retain **2,626 identical objects** and **89 unchanged failures**.
All fourteen configured GX objects remain identical. All ten AX units compile;
eight are identical, while AXSPB and AXAux change. Only `__AXProcessAux` changes
inside AXAux, and all thirteen AXAux functions pass **6,656 three-way native
comparisons** against a fresh reference and the original executable, including
callback effects. Backend tests pass **1,619**, with the previously confirmed
embedded-assembly failure excluded.

Artifacts are under `target/magic-load-{canaries,old-guards,old-inline,old-inputs,real,aux,index,recent,previous,library,ax-library}`.
`target/magic-load-final-verification.json` pins the compiler/harness and
verifies **439 execution-tested object hashes**. Full-project compilation,
linking, and matching across versions remain unfinished; these focused counts
are not a corpus parity estimate.

## Retained global input values, 2026-09-08

The complete BfBB `__AXPrintStudio` drops **1,052 → 1,016 bytes** by retaining
all nine initial input values through their local-only clamp branches and
volume stores. It now matches the reference size while retaining the nine
required narrow conversions from the preceding milestone. Register choices and
instruction scheduling still differ: AXSPB remains **4/5 exact candidate
functions**, **5/5 exact fresh reference functions**, and zero unresolved
relocations against the pinned original DOL. All five functions pass **5,120
three-way native comparisons** against fresh GC/1.2.5n, the original executable,
and the independent fade/accumulation model.

A separate structured-tree pass captures repeated nonvolatile word-global
operands at an existing unconditional read. The capture uses an ordinary typed
local, so register placement and lifetime remain allocator decisions. Reuse
follows branches that only change unescaped locals; both arms must preserve
memory for reuse after a join. The first store can consume the saved value,
then invalidates it. Calls, other memory writes, labels, and unmodeled control
flow also end reuse. Volatile inputs and short-circuit seed expressions are
excluded. The pass runs from O2, after existing whole-expression reuse, and
leaves compiler-version policy outside the transformation.

Canaries **2044–2047** cover clamping, local-only joins, pointer stores and global
writes in one branch, calls, volatile reads, and straight-line uses at O0, O1,
O2, and O4. Baseline, candidate, and references compile **60/60 objects** across
fifteen builds. Global input load/store symbol-reference counts match in all
**420 functions**. Exact output remains **0/60 whole objects** and **0/420
function text plus symbolic relocation comparisons** in these samples.
All **107,520 paired native comparisons** pass; baseline and references also
have no failures. The checks include aliased and distinct output pointers,
signed boundaries, branch flags, callbacks that change input and clobber
volatile registers, changing volatile reads, all neighboring bytes, saved
GPR/FPR images, and SP/LR/PC.

The sixty changed older objects from canaries 2022, 2029, 2032, and 2033 pass
another **153,600 paired native comparisons**. The other **570/630 objects** in
the 2002–2043 panel remain identical. The indexed panel retains **1,114 identical
compiled objects** and **972 known exact matches** among 1,674 rows. Canaries
1821–2001 retain **2,626 identical objects** and **89 unchanged failures**. All
fourteen configured GX objects and the other nine AX objects remain identical;
all ten AX units compile. Backend tests pass **1,616**, with the previously
confirmed embedded-assembly failure excluded.

Artifacts are under `target/retained-input-{canaries,old-guards,old-inline,real,index,recent,previous,library,ax-library}`.
`target/retained-input-final-verification.json` pins the compiler/harness and
verifies **347 execution-tested object hashes** after the final rebuild.
Full-project compilation, linking, and matching across versions remain
unfinished; these focused counts are not a corpus parity estimate.

## Constant multiplication before narrow stores, 2026-09-07

Canaries **2040–2043** now match **60/60 whole reference objects** and
**1,080/1,080 function text plus symbolic relocation comparisons** across fifteen
compiler builds at O0, O1, O2, and O4. The preceding compiler matches none of
these whole objects. Each sample covers eighteen signed and unsigned byte/halfword
stores, identity and zero products, both constant operand positions, explicit
casts, compound inputs, and positive/negative factors including 1, 2, 3, 7, 8,
and 15. All **276,480 paired native comparisons** pass independent memory and
ABI checks; baseline and references also have no execution failures. The final
three register-placement changes pass another **13,824 paired comparisons**.

A dedicated assignment-conversion policy distinguishes constant products from
ordinary narrow stores. Early GameCube builds retain signed extensions after
identity, optimized negation, and positive-power-of-two rewrites, while raw
products and later negative-power rewrites omit them. Newer builds preserve
product conversions at O0 and eliminate them starting at O1. Separate version
profiles select immediate multiplication or shift/subtract sequences for small
factors one below a power of two. Compound shift/subtract operands receive a
separate virtual home so both instructions can consume the original value.
The selector is limited to supported word integer operands and narrow stores;
zero folding requires a register leaf without a frame slot.

The complete BfBB `__AXPrintStudio` now retains all nine reference sign extensions
after `delta * -1`. Its size increases **1,016 → 1,052 bytes**, against a
**1,016-byte** reference: the previous equal size concealed missing conversions
and redundant input reloads. AXSPB remains **4/5 exact candidate functions** and
**5/5 exact fresh reference functions**, with no unresolved relocations against
the pinned original DOL. All five functions pass **5,120 three-way native
comparisons** against fresh GC/1.2.5n, the original executable, and the independent
fade/accumulation model. Input reloads and instruction placement remain work for
the next matching pass.

The focused regression panels retain **1,114 identical compiled indexed objects**
including **972 known exact matches**, **2,626 identical recent objects** with
**89 unchanged failures**, and **570/570 identical objects** from canaries
2002–2039. All fourteen configured GX objects and the other nine AX objects are
identical; all ten AX units compile. Backend tests pass **1,612**, with the
previously confirmed embedded-assembly failure excluded; all **51 version tests**
pass.

Artifacts are under `target/narrow-multiply-{canaries,real,index,recent,previous,library,ax-library}`.
`target/narrow-multiply-final-verification.json` pins the compiler/harness and
**182 execution-tested object hashes**. These are focused diagnostic counts;
full-project compilation, linking, and matching across versions remain unfinished.

## Terminal object arguments and preserved signed dividends, 2026-09-07

The complete BfBB `__AXPrintStudio` function now has the reference's **1,016-byte
size**, down from 1,024 bytes. Its retained studio address survives through
placement of the final flush argument, and the allocator can use r3 for that
address. This is **not an exact-output match**: input reloads, narrow conversions,
and instruction scheduling still differ. AXSPB retains **4/5 exact candidate
functions**, **5/5 exact fresh reference functions**, and no unresolved
relocations against the pinned original DOL. All five functions pass **5,120
three-way native comparisons** against fresh GC/1.2.5n, the original executable,
and the independent fade/accumulation model.

The existing global-base planner counts complete-object address arguments at a
terminal call when there is no preceding or nested call. Those arguments use
the address before the call's clobbers; they do not introduce a saved live range.
Address emission gives the virtual base a preference for its actual argument
register. Signed division's constant high half receives a virtual home while
such an object base is live, removing an artificial fixed-register conflict.
The allocator still decides whether an argument-register preference is legal.

The new parameter probes also expose and fix an existing runtime bug: constant
signed division overwrote a named dividend with its quotient or sign bit, so
later source reads observed the intermediate value. A named source now keeps
its value while a separate virtual temporary holds the quotient/sign correction,
unless the requested destination is that source's own home. Dead input homes
can still be reused by normal allocation.

Canaries **2038–2039** cover O4 and explicit O0, terminal argument positions,
duplicate pointers, live input parameters, nested callback arguments, and
signed divisors **3, 7, and 160**. Baseline, candidate, and references compile
**30/30 objects** across fifteen builds. All **53,760 paired native comparisons**
pass independent models; the baseline has **22,080 failing cases** in the same
panel, while the candidate and references have none. Checks include signed
boundaries, repeated dividend reads, every packet and neighboring byte,
callback order, argument values and writes, volatile register clobbers, saved
GPR/FPR images, and SP/LR/PC. Exact matching remains **0/30 whole objects** and
**0/210 function text plus symbolic relocation comparisons** in the new samples.

The sixty changed objects from canaries 2034–2037 pass another **38,400 paired
native comparisons**. The other **480/540** objects in the 2002–2037 panel remain
identical. The indexed panel retains **1,114 identical compiled objects** and
**972 known exact matches** among 1,674 rows. Canaries 1821–2001 retain **2,626
identical objects** and **89 unchanged failures**. All fourteen configured GX
objects and the other nine configured AX objects remain identical; all ten AX
units compile. Backend tests pass **1,610**, with the previously confirmed
embedded-assembly failure excluded.

Artifacts are under `target/terminal-base-{canaries,old-address-canaries,old-index-canaries,real,index,recent,previous,library,ax-library}`.
`target/terminal-base-final-verification.json` pins the compiler and harness
fingerprints and verifies **272 execution-tested object hashes** after the final
rebuild. Full-project compilation, linking, and compiler-version matching remain
unfinished; these focused counts are not a corpus parity estimate.

## Folding captured member pointers into displacements, 2026-09-07

The complete BfBB `__AXPrintStudio` function drops from **1,096 to 1,024 bytes**;
the fresh GC/1.2.5n reference remains **1,016 bytes**. Removing the two captured
member-pointer computations per channel leaves integer loads and stores using
the retained studio base directly. The eight-byte size gap does not imply an
eight-byte instruction mismatch: input reloads, narrow conversions, register
allocation, scheduling, and final address rematerialization still differ.
AXSPB retains **4/5 exact candidate functions**, **5/5 exact fresh reference
functions**, and no unresolved relocations against the pinned original DOL.
All five functions pass **5,120 three-way native comparisons** against fresh
MWCC, the original executable, and the independent fade/accumulation model.

A separate pass runs before scheduling and allocation. It folds an address
move/add from the existing retained global base only when the destination is a
virtual register with one dominating definition and every use is an integer
memory base. It preserves the memory opcode and checked signed displacement.
Escaping values, overwritten aliases or bases, update-form accesses, relocated
instructions, overflowing displacements, bypassed definitions, back edges,
jump tables, and opaque instruction streams remain outside the fold. Removal
uses the shared branch, label, and relocation retargeting utility. The existing
allocator owns the resulting base live range; no physical register rewrite or
new version policy is introduced.

Canaries **2036–2037** cover O4 and explicit O0, signed halfword and unsigned byte
loads, word/halfword/byte updates, and positive and negative pointer indices
through inline pointer parameters. Baseline, candidate, and fresh references
compile **30/30 objects** across fifteen builds. All **15,360 paired native
comparisons** pass independent models, including overflow and narrowing edges,
every packet byte, callback observations and writes, volatile register
clobbers, saved GPR/FPR images, and SP/LR/PC. The baseline also passes. Exact
matching remains **0/30 whole objects** and **0/30 function text plus symbolic
relocation comparisons** in the new samples.

The thirty changed objects from canaries **2034–2035** pass another **23,040
paired native comparisons**. The other **480/510** objects in the 2002–2035
panel remain identical. The indexed panel retains **1,114 identical compiled
objects** and **972 known exact matches** among 1,674 rows. Canaries 1821–2001
retain **2,626 identical objects** and **89 unchanged failures**. All fourteen
configured GX objects and the other nine configured AX objects remain identical;
all ten AX units compile. Backend tests pass **1,608**, with the previously
confirmed embedded-assembly failure excluded. Four focused pass tests also
pass after the final cleanup.

Artifacts are under `target/member-displacement-{canaries,old-canaries,real,index,recent,previous,library,ax-library}`.
`target/member-displacement-final-verification.json` pins the compiler and
harness fingerprints and verifies **182 execution-tested object hashes** after
the final rebuild. Full-project compilation, linking, and compiler-version
matching remain unfinished; these focused counts are not a corpus parity estimate.

## Reusing retained global bases for member addresses, 2026-09-07

The complete BfBB `__AXPrintStudio` function drops from **1,232 to 1,096 bytes**;
the fresh GC/1.2.5n reference remains **1,016 bytes**. AXSPB retains **4/5 exact
candidate functions**, **5/5 exact fresh reference functions**, and no unresolved
relocations against the pinned original DOL. All five functions pass **5,120
three-way PowerPC execution comparisons** against fresh MWCC, the original
executable, and the independent fade/accumulation model. Register allocation,
scheduling, and other instruction differences in `__AXPrintStudio` remain open.

Member address emission now consumes the existing retained global-base cache,
just as member loads and stores already do. It derives the address with a move
or signed displacement add; the shared base itself remains intact. Offsets
outside the signed 16-bit range retain the full address materialization path.
The existing planner still owns the base's lifetime and whether it crosses a
call. The change adds no new alias pass or version policy.

Canaries **2034–2035** cover O4 and explicit O0, inline word writes through
addresses of halfword members, zero and nonzero offsets, conditional callbacks,
and member offsets at 32 KiB and beyond. Baseline, candidate, and references
all compile **30/30 objects** across fifteen builds. All **23,040 paired native
comparisons** pass independent models; the baseline also passes. Checks include
all packet and large-object bytes, unchanged inputs, callback pointers and
writes, volatile register clobbers, saved GPR/FPR images, and SP/LR/PC. Exact
matching remains **0/30 whole objects** and **0/90 function text plus symbolic
relocation comparisons** in these new samples.

Focused regression checks retain **1,114 identical compiled indexed objects**
and **972 known exact matches** among 1,674 rows. Canaries 1821–2001 retain
**2,626 identical objects** and **89 unchanged failures**; canaries 2002–2033
retain **480 identical objects**. All fourteen configured GX units remain
identical, and all ten configured AX units compile, with the other nine AX
objects identical. Backend tests pass **1,604**, with the previously confirmed
embedded-assembly failure excluded.

Artifacts are under `target/static-member-alias-{canaries,real,index,recent,previous,library,ax-library}`.
`target/static-member-alias-final-verification.json` pins the compiler and
harness fingerprints and verifies **92 execution-tested object hashes** after
the final rebuild. Full-project compilation, linking, and compiler-version
matching remain unfinished; these focused counts are not a corpus parity estimate.

## Reusing guarded integer values, 2026-09-07

The complete BfBB `__AXPrintStudio` function drops from **1,520 to 1,232 bytes**
by sharing each guard's signed quotient with its body and recording the zero
test on the quotient's final correction add. The fresh GC/1.2.5n reference is
**1,016 bytes**. AXSPB retains **4/5 exact candidate functions**, **5/5 exact
fresh reference functions**, and no unresolved relocations against the pinned
original DOL. All five functions retain **5,120 three-way PowerPC execution
comparisons** against fresh MWCC, the original executable, and an independent
fade/accumulation model. This is progress toward the remaining mismatch, not an
exact-output claim for `__AXPrintStudio`.

A separate semantic pass tracks available named word computations through
straight-line assignments and into dominated if arms. It reuses only equal
expressions held in nonvolatile, unescaped locals with the same word type.
Stores, calls, carrier/source reassignments, labels, and control-flow joins
invalidate facts. Pointer loads and floating expressions are outside this
pass. Existing liveness and register allocation own the resulting local copies.
Reference probes place the reuse boundary at **O2**: O0/O1 repeat the quotient,
while O2/O3/O4 share it. All fifteen measured builds confirm that boundary.

A separate instruction fold records the final signed-quotient correction add
when the next instructions compare that same result with zero and branch on
CR0 equality. It rejects an independently reachable compare, jump-table bodies,
and verbatim instruction streams; removal uses the existing branch, label, and
relocation retargeting utility after symbolic edges have been resolved. O0
preserves its separate compare.

Canaries **2029–2033** cover all five optimization levels, repeated guarded
computations, intervening global writes and callbacks, overwritten carriers,
volatile input that changes between reads, and narrow intermediate values.
Both compilers produce **75/75 objects**. The guarded quotient counts agree
**75/75** (two at O0/O1, one at O2/O3/O4). All **230,400 paired native comparisons**
pass independent models, including volatile read counts and observed values,
callback effects and register clobbers, every global and neighboring byte,
saved GPR/FPR images, and SP/LR/PC. Baseline execution also passes; this milestone
improves optimization fidelity rather than closing a runtime miscompile.
There are still **0/75 whole objects** and **0/450 function text plus symbolic
relocation comparisons** exact; allocation, scheduling, and lower-level
division selection remain open in these samples.

The fifteen O4 objects from inline canary 2022 also change, and the 2022–2023
panel passes **30,720 paired native comparisons** after remeasurement. The
other **390/405** objects in the 2002–2028 panel remain identical, including all
**75 exact accumulator objects** from the preceding milestone. The indexed
panel retains **1,114 identical compiled objects** and **972 known exact
matches** among 1,674 rows. Canaries 1821–2001 retain **2,626 identical objects**
and **89 unchanged failures**; all fourteen complete GX objects and the other
nine complete AX objects remain identical. Backend tests pass **1,604**, with
the previously confirmed embedded-assembly failure excluded.

Artifacts are under `target/guarded-values-{canaries,inline-canaries,real,index,recent,previous,library,ax-library}`.
`target/guarded-values-final-verification.json` pins the compiler, harness, and
execution-tested object hashes. Full-project compilation, linking, and
compiler-version matching remain unfinished; these focused counts are not a
corpus-wide parity estimate.

## Matching accumulator schedules across GameCube and Wii, 2026-09-07

The complete BfBB `__AXDepopVoice` function now matches both fresh GC/1.2.5n
output and the original DOL exactly. AXSPB improves **3/5 to 4/5 exact
functions**, with **5/5 fresh reference functions exact** and no unresolved
relocations. `__AXPrintStudio` remains 1,520 versus 1,016 bytes; full-project
build and matching work remains unfinished.

The shared DAG scheduler now separates physical staging order from value
critical-path dependencies. Ready stores and arithmetic lead pending loads in
the accumulator policy, letting independent loads fill the remaining issue
slots. A checked reverse interval allocator reserves explicit physical homes
and colors other values in reverse emission order. It allows genuine
producer/consumer handoffs and rejects overlapping fixed values or an
exhausted register pool, rather than assigning an arbitrary occupied register.
The existing general DAG policies continue through the same scheduler with no
additional ordering edges.

GameCube uses paired issue and a one-step r0 handoff; Wii uses single issue and
a two-step handoff. This difference is a named profile policy and an observable
active quirk. Unoptimized or explicitly disabled scheduling retains source
order. These are measured compiler issue models, not hardware cycle claims.

New canaries **2026–2028** exercise two through nine accumulators, alternating
signed/unsigned halfwords at nonzero member offsets, reversed field order,
O0/O4, and `-schedule off`. Together with canaries 2024–2025, the focused panel
improves **0/75 to 75/75 whole objects exact** and produces **420/420 exact
function text plus symbolic relocation comparisons** across all fifteen
measured builds. Both compilers emit **75/75 objects** with no unknown outcomes.
The O0 samples carry explicit optimization flags in their corpus metadata.

All **215,040 paired PowerPC execution comparisons** pass independent models,
checking every accumulator, neighboring memory, unchanged packets, saved
GPR/FPR images, and SP/LR/PC. The complete AXSPB unit also retains **5,120
three-way comparisons** against fresh MWCC and the original DOL, including fade
boundaries and flush events. This remains bounded native integration evidence.

Focused regression checks preserve all **1,114 compiled indexed objects** and
**972 known exact matches** among 1,674 rows. Canaries 1821–2001 retain **2,626
identical objects** and **89 unchanged failures**; all **330 objects** from
2002–2023, all fourteen complete GX objects, and the other nine complete AX
objects remain identical. All ten configured AX sources still compile.
Backend tests pass **1,601** with the previously confirmed embedded-assembly
failure excluded; register/scheduler tests pass **107** with eight existing
ignored tests; version-policy tests pass **49**.

Artifacts are under `target/accumulator-schedule-{canaries,real,index,recent,previous,library,ax-library}`.
`target/accumulator-schedule-final-verification.json` records the compiler,
harness, and execution-tested object hashes. These targeted measurements do
not establish full-corpus or full-project parity.

## Complete AXSPB translation unit and AX library compilation, 2026-09-07

BfBB's unchanged, fully configured **AXSPB.c now compiles**, bringing complete
AX translation-unit coverage from **9/10 to 10/10**. All five functions pass
**5,120 three-way PowerPC execution comparisons** against fresh GC/1.2.5n
output and the original DOL, with an independent signed fade/accumulation model.
Cases include signed extrema, truncation around ±160, clamping around ±3200,
nine distinct channels, signed halfword inputs, and wrapping word additions.
Checks include the complete studio buffer and surrounding bytes, every unit
owned global, unchanged input packets, flush arguments and bytes at call time,
return values, preserved GPR/FPR registers, and SP/LR/PC. `DCFlushRange` is modeled
with volatile-register clobbers; this is bounded integration evidence.

Inline argument materialization now accepts side-effect-free addresses through
pointer casts, capturing them once in the existing hygienic parameter lanes.
The shared leaf structured emitter admits generated inline-return labels and
resolves their forward branches before scheduling. Consecutive inline calls
therefore retain separate early-return boundaries.

The existing dependency-DAG emitter now handles independent word accumulators
fed by signed or unsigned halfword fields of a pointer parameter. Global reads
must refer to each store's own distinct nonvolatile target. Field update chains
carry explicit staging dependencies, and hoisted accumulator loads exclude r0.
Native execution caught an initial last-channel overwrite when both live inputs
received r0; the staging and register constraints close all 1,024 full-unit
mismatches. Admission is capped at nine accumulators, matching the available
volatile register homes. Larger or cross-dependent runs still defer.

Canaries **2022–2025** cover casted global/member addresses, multiple inline
instances, signed division and clamping, and signed/unsigned field accumulation.
Candidate compilation improves **0/60 to 60/60 objects** across fifteen builds
at O0/O4; fresh reference compilation is **60/60**. All **61,440 paired native
comparisons** pass, with **0/60 whole objects** and **0/120 function text plus
symbolic relocation comparisons** exact. Compilation and execution progress
here does not establish byte parity.

The pinned layout `docs/reference-layouts/bfbb-axspb.json` gives **5/5 fresh
reference functions exact** against the original DOL and **3/5 candidate
functions exact** (`__AXGetStudio`, `__AXSPBInit`, `__AXSPBQuit`), with no unresolved
relocations. The reference measurement adds its anonymous `...bss.0` anchor
through the verified `__AXStudio` object; the candidate has no anonymous anchor.
`__AXPrintStudio` remains 1,520 versus 1,016 bytes, and `__AXDepopVoice` remains
148 bytes with a different schedule/register assignment.

Focused regression checks preserve all **1,114 compiled objects** in the
**1,674-row indexed panel**, including **972 known exact matches**. The recent
1821–2001 panel retains **2,626 identical objects** and **89 unchanged failures**;
all **300** objects from canaries 2002–2021, all fourteen complete GX objects,
and all nine previously compiling AX objects remain identical. Backend tests
pass **1,601**, excluding the previously confirmed embedded-assembly failure;
DOL tooling tests pass **26**.

Artifacts are under `target/depop-inline-{canaries,real,index,recent,previous,library,ax-library}`.
`target/depop-inline-final-verification.json` records the final compiler and
execution-tested object hashes. All configured AX sources now compile, but
full-project builds, linking, and compiler-version matching remain unfinished.
These focused counts are not a corpus-wide parity estimate.

## Complete AXOut translation unit and embedded callback lifetimes, 2026-09-07

BfBB's unchanged, fully configured **AXOut.c now compiles**, increasing complete
AX translation-unit coverage from **8/10 to 9/10**. All nine functions pass
**2,304 three-way PowerPC execution comparisons** against a fresh GC/1.2.5n
reference object and the original DOL. This covers frame construction, mailbox
polling, profile recording/copying, DMA callbacks, DSP resume, initialization,
shutdown, and callback registration. Internal calls execute the actual compiled
bodies, including their inline expansions. External SDK helpers model polling
progress, clock values, initialization flags, interrupt tokens, callback effects,
and volatile-register clobbers. Comparisons cover ordered helper calls, every
unit-owned global and buffer, copied profiles, saved GPR/FPR images, SP/LR/PC,
and the observed registration return register. DSP callback addresses are
normalized by function identity. This is bounded integration evidence.

The existing guarded table-entry emitter now also handles a nonvolatile global
callback with no arguments inside a larger structured body. It loads the callee
once, tests it in r12, and carries it across the true edge using the existing
version-specific indirect-call convention. It does not retain the callback
after the call; replacement or clearing is observed by the next guard.

The existing wide call-result store path now accepts a member of a global
aggregate. Both EABI result words remain reserved while the destination address
is formed. A word-sized cast of a wide call-result difference uses the low
subtraction; volatile timestamp globals still receive both ordered word reads.
The typed return path preserves that cast instead of stripping it as an integer
identity.

Full-unit execution also found a prologue dependency defect: a cached task
address was formed from the incoming r31 before the BSS anchor initialized it.
Structured prologue emission now initializes an anchor before caches that
consume it, while retaining the ordering of independent initialization paths.
This closes all **256** initial `__AXOutInitDSP` execution mismatches, where
`DSPAddTask` received an address derived from the caller's saved register.

Canaries **2016–2021** cover embedded callbacks, callback replacement between
guards, global timestamp members, and truncated volatile/nonvolatile timestamp
differences. Candidate compilation improves **0/90 to 90/90 objects** across
fifteen builds at O0/O4; fresh reference compilation is **90/90**, with no unknown
outcomes. There are **61/240 exact function text plus symbolic relocation
comparisons** and **10/90 whole objects exact**. All **122,880 paired execution
comparisons** pass against independent expected results, including volatile
read order and globals modified by the clock call before subtraction.

The pinned [AXOut layout](reference-layouts/bfbb-axout.json) verifies **9/9
fresh-reference functions** against the original DOL with no unresolved
relocations. Candidate linked text is exact for **3/9** functions:
`__AXDSPInitCallback`, `__AXDSPDoneCallback`, and `AXRegisterCallback`. The
candidate object is **6,096 bytes**, including **1,624 bytes of text**, versus
the reference's **6,184 bytes** and **2,000 bytes of text**. Initialization-loop
unrolling and several instruction schedules still differ.

Regression checks retain all **1,114** compiling indexed objects byte-for-byte,
including **972** known exact matches. All **2,626/2,715** compiling recent
source/build pairs remain unchanged; **89** keep their failures. All **210**
canaries from the preceding two milestones, fourteen full GX objects, and all
eight previously compiling AX objects remain unchanged. Backend tests pass
**1,601**, excluding the previously confirmed embedded-assembly failure; DOL
tooling tests pass **26**.

Artifacts are under `target/frame-callback-{canaries,real,index,recent,previous,library,ax-library}`;
`target/frame-callback-final-verification.json` records the final compiler and
execution-tested object hashes. The remaining AX compile failure is
`AXSPB.c`, which needs retained `__AXDepopFade` inline expansion. Full-project
compilation and matching remain unfinished; these focused counts are not a
corpus-wide parity estimate.

## Complete AXCL translation unit and global array addressing, 2026-09-07

BfBB's unchanged, fully configured **AXCL.c now compiles**, increasing complete
AX translation-unit coverage from **7/10 to 8/10**. All five command-list
functions pass **5,120 three-way PowerPC execution comparisons** against a
fresh GC/1.2.5n reference object and the original DOL, also checked against an
independent command-stream model. Cases cover both command buffers, mono,
stereo, DPL2 and default modes, enabled/disabled auxiliary inputs, compressor
settings, and split high/low address words. Comparisons include all 1,536
command-buffer bytes, cursor and cycle globals, ordered helper calls, saved
GPR/FPR images, and SP/LR/PC. External SDK helpers are modeled with their
return/output values and volatile-register clobbers; this is bounded
integration evidence.

The existing dependency-DAG scheduler now accepts pure runs of word constants
and static symbol addresses. HA/LO and small-data address nodes use the same
version-selected scheduling and register allocation as integer expressions;
repeated constants share a live value within these initialization runs.
Pointer-address high halves exclude r0 when consumed by `addi`. This produces
an exact **36-byte `__AXClInit`** without a function-specific instruction packet.

Execution testing exposed two additional correctness defects. Global pointer
arithmetic now loads the pointer object and scales constant or loaded offsets
by its pointee size; a halfword cursor advances two bytes. Zero-offset global
snapshots retain their existing lowering. The parser retains global arrays'
inner dimensions and normalizes successive subscripts into a scalar storage
index, preserving address-valued partial rows and their extent for bounded
`sizeof` queries. This fixes row selection without introducing pointer loads
from embedded array storage.

Canaries **2010–2015** cover address/constant initialization, cursor increments
and decrements, word and struct strides, loaded offsets, matrix reads/writes,
row addresses and sizes, and double-buffer exchange. Candidate compilation
improves **30/90 to 90/90 objects** across fifteen builds at O0/O4; fresh
reference compilation is **90/90**, with no unknown outcomes. There are
**194/450 exact function text plus symbolic relocation comparisons** and
**0/90 whole objects exact**. All **115,200 paired execution comparisons**
pass against independent expected results.

The same array fix repairs buffer addresses in the already-compiling
**AXAux.c**: all thirteen functions pass **6,656 three-way execution
comparisons**, including auxiliary callbacks, buffer mutation, cache-operation
order, and ring-position updates. The reference's BSS anchor is mapped only
after checking both buffer placements and their 5,760-byte separation.
**AXProf.c** passes **2,048 three-way comparisons** against its independent
profile-ring model; the frozen baseline has **1,160 mismatches** on those
cases because it fails to scale the profile index by the 56-byte record size.

The pinned [AXCL layout](reference-layouts/bfbb-axcl.json) verifies **5/5
fresh-reference functions** against the original DOL with no unresolved
relocations. Candidate linked text is exact for **3/5** functions:
`__AXGetCommandListCycles`, `__AXClInit`, and `__AXClQuit`. Its object is
**6,200 bytes**, including **1,884 bytes of text**, versus the reference's
**6,072 bytes** and **1,820 bytes of text**. The command builder and buffer
exchange still differ in scheduling and register allocation.

Regression checks retain all **1,114** compiling indexed objects byte-for-byte,
including **972** known exact matches. All **2,626/2,715** compiling recent
source/build pairs remain unchanged; the other **89** keep their failures.
All **120** preceding milestone canaries, fourteen full GX objects, and five
unaffected AX objects remain unchanged. Parser tests pass **409**, excluding
two failures reproduced on the frozen baseline; backend tests pass **1,601**,
excluding the previously confirmed embedded-assembly failure. DOL tooling
tests pass **26**. Final recompilation verifies **93** execution-panel object
hashes after the final source changes.

Artifacts are under `target/ax-command-{canaries,real,aux,prof,index,recent,previous,library,ax-library}`;
`target/ax-command-final-verification.json` records the final compiler and
verified object hashes. The remaining AX compile failures are `AXOut.c`
(global value reuse across a conditional) and `AXSPB.c` (retained
`__AXDepopFade` inline expansion). Full-project compilation and matching
remain unfinished; these focused counts are not a corpus-wide parity estimate.

## Complete AXAlloc translation unit and array value lifetimes, 2026-09-07

BfBB's unchanged, fully configured **AXAlloc.c now compiles**, increasing
complete AX translation-unit coverage from **6/10 to 7/10**. All ten allocator
functions pass **10,240 three-way PowerPC execution comparisons** against a
fresh GC/1.2.5n reference object and the original DOL. This includes callback
stack draining, all doubly linked removal positions, free-list allocation,
lower-priority tail stealing, exhausted acquisition, and interrupt-state
restoration. Calls among allocator functions execute their actual compiled
bodies. Three external SDK helpers and the user callback are modeled with
ordered events, volatile-register clobbers, and memory effects. Comparisons
cover return values, 4,608 bytes of voice storage, all three allocator globals,
saved GPR/FPR images, and SP/LR/PC; this is bounded integration evidence.

Global-array assignment expressions now retain their converted value in an
explicit virtual register through element-address formation. This supports
chained assignments and computed integer stores without assuming that a store
leaves its result in r0. Narrow assignment results preserve their conversion
before an outer word store or return. Leaf functions with local assignments
and a small conditional memory update can use the existing structured CFG
lowerer. Member-address and member-store lowering distinguish a pointer table
from an array of embedded structs: they load the selected pointer before
applying the member offset. Existing member-load schedules remain intact.
Finally, integer branch comparisons preserve a scratch-held first operand
before evaluating a second nonconstant, non-register operand. This prevents
two table reads from degenerating into `cmplw r0,r0`.

Canaries **2002–2009** cover assigned values, signed-byte/halfword narrowing,
call and member results, computed stores, conditional pointer snapshots,
member addresses, member writes after replacing a table entry, and comparisons
of loaded operands. Candidate compilation improves **30/120 to 120/120
objects** across fifteen builds at O0/O4; fresh reference compilation is
**120/120**. Reference output filenames are kept short because several older
compilers fail with the longer harness output paths. There are **48/600 exact
function text plus symbolic relocation comparisons**, with **0/120 whole
objects exact**. All **153,600 paired execution comparisons** pass against
independent expected models. On the 30 already-compiling comparison objects,
the frozen baseline has **9,420 execution mismatches** in 23,040 cases; the
new compiler has none.

The pinned [AXAlloc layout](reference-layouts/bfbb-axalloc.json) verifies
**10/10 fresh-reference functions** against the original DOL, including the
BSS section anchor validated through `__AXStackHead`. Candidate linked text is
exact for **2/10** functions (`__AXGetStackHead`, `__AXPushCallbackStack`). The
candidate object is **3,464 bytes** with **1,360 bytes of text**, versus the
reference's **3,024 bytes** and **1,224 bytes of text**. Instruction scheduling
and loop unrolling still differ despite the execution agreement.

Regression checks retain **1,114** compiling indexed objects byte-for-byte,
including all **972** known exact matches. All **2,626** compiling recent
source/build pairs remain byte-identical; **89** keep their compilation
failures. All fourteen full GX objects, AXVPB, and the six previously compiling
AX units remain unchanged. Compiler library tests pass **1,601**, excluding
the previously confirmed embedded-assembly failure; DOL tooling tests pass
**26**. Final recompilation verifies **143** execution-panel and full-unit
object hashes after the final source changes.

Artifacts are under `target/array-assignment-{canaries,real,index,recent,library,full-ax,ax-library}`;
`target/array-assignment-final-verification.json` records the final compiler
and verified object hashes. The three remaining AX compile failures are
`AXCL.c` (global initialization scheduling), `AXOut.c` (a global value reused
across a conditional), and `AXSPB.c` (retained `__AXDepopFade` inline expansion).
Full-project compilation and matching remain unfinished; these focused counts
are not a corpus-wide parity estimate.

## GC/1.1p1 O0 polling register scopes and spill overlap, 2026-09-07

The **1,024 polling execution differences** exposed by the GXMisc canaries
are closed. Both functions in GC/1.1p1 O0 canary 1985 now match their complete
reference bodies and relocations: `wait_twice` is **204 bytes** and
`guarded_settle` is **160 bytes**. The **entire object matches**, including
symbol ordering and metadata.

A bounded lowering path under the existing unoptimized source-spill policy
plans register homes before wide frame-value lowering. It verifies each
inlined pair against the retained callee's two local declarations, assigns
each invocation a separate four-GPR block, and preserves declaration order
within that block. Word locals consume descending homes in first-definition
order. Shared helper-frame emission now accepts an explicit linkage
convention while preserving existing callers' default sequences. The actual
SP+8 parameter store overwrites the lowest helper-saved register: r24 in
`wait_twice`, r26 in `guarded_settle`. The epilogue therefore reproduces the
reference's saved-register corruption, including when the guard is false.
Recognition declines unverified scopes, unsupported control flow, volatile or
addressable locals, nonconstant thresholds, and register plans outside the
measured aligned frame shape. Unoptimized object emission registers generated
save/restore helpers before the current function, retaining ordinary body-call
and assembly-definition ordering.

Canaries **2000–2001** add unsigned and signed guards, reversed wide-local
declarations, reversed scalar-local declarations, thresholds 7 and 19, and
three separate inline invocations. Fresh candidate and reference compilation
both pass **30/30 objects** across fifteen builds at O0/O4. All five functions
and the entire GC/1.1p1 O0 object are exact: **1/30 whole-object matches** and
**5/150 exact function text plus symbolic relocation comparisons**. The
preceding 1984–1985 panel also compiles **30/30**, with **1/30 whole-object
matches** and **2/60 exact function comparisons**.

All **107,520 paired PowerPC execution comparisons** pass: **76,800** new
cases and **30,720** preceding-panel cases. Independent models check ordered
clock/counter calls, signed elapsed comparisons around word carries and
64-bit wrap boundaries, guard outcomes, saved GPR/FPR images, SP/LR/PC, and
unrelated memory. Helper callbacks clobber volatile registers; the original
DOL's save/restore helper instructions execute on both sides. The models
include the source-spill corruption on the affected build.

Regression checks retain **1,114** compiling indexed objects byte-for-byte,
including all **972** known exact matches. Among **2,685** recent source/build
pairs, **2,595** objects are unchanged, **89** retain compilation failures,
and the one changed object is the newly exact canary 1985. All fourteen full
GX objects and the AX voice-parameter object remain byte-identical. Compiler
library tests pass **1,601**, excluding the previously confirmed embedded-asm
failure; object-writer tests pass **40**. Final recompilation verifies **75**
execution-panel and full-unit object hashes after the last source changes.

Artifacts are under `target/polling-spill-{canaries,previous-canaries,index,recent,library,full-ax}`;
`target/polling-spill-final-verification.json` records the final compiler and
verified object hashes. These are focused measurements. Broader instruction
matching, register allocation, and full-project parity remain unfinished.

## GC/1.1p1 O0 indirect-call parameter spills, 2026-09-07

The **512 callback execution differences** exposed by the GXTexture canaries
below are now closed. GC/1.1p1 O0 `member_callback` matches its reference's
complete **76-byte** function body and symbolic relocations, including loading
the wrong callback address after both parameters are spilled at SP+8.

The existing version-specific source-spill policy now admits a single indirect
call through one pointer parameter, passing a word member from the other.
Both source images are stored in declaration order and reloaded independently;
the later pointer therefore supplies both the argument and callee loads. A
pointer result local retains r31, while a directly consumed call result stays
in r3. The latter form has an eight-byte frame, so its SP+8 parameter store
also overwrites the caller's backchain. These are intentional reproductions of
observed compiler behavior. Recognition excludes additional calls, post-call
parameter reads, floating results, and unsupported local/control-flow shapes.
Emission reuses the existing linkage-first prologue, indirect branch emitter,
expression lowerer, and epilogue. Ordinary lowering also accepts an explicitly
typed indirect-call result as a member-access base, allowing the direct-result
canary to compile on the other versions.

Canaries **1998–1999** vary declaration order, member offsets, initialized and
assigned pointer locals, post-call arithmetic, and direct result consumption.
Candidate compilation improves **0/30 to 30/30 objects** across fifteen builds
at O0/O4; fresh reference compilation is **30/30**. The entire GC/1.1p1 O0
object matches, for **1/30 whole-object matches** and **18/150 exact function
text plus symbolic relocation comparisons**. All five functions in that O0
object are exact. The preceding 150-object texture panel retains **22/150
whole-object matches**, with exact function comparisons increasing **82/450
to 83/450**.

All **384,000 paired PowerPC execution comparisons** agree: **153,600** in the
new panel and **230,400** in the preceding panel. Independent expected models
include the erroneous callback target and argument, return bits, callback
memory effects, saved GPR/FPR images, caller linkage words, and SP/LR/PC. The
new panel exercises both successful wrong callbacks and **1,024** expected
unmapped-target faults. The preceding panel includes the original **512**
fault cases. Its exact, relocation-free `member_callback` body is placed at
one common link address on both sides so the fault LR is comparable despite
the preceding function's different size; each absolute fault state is also
checked against its model. Callback helpers clobber volatile registers.

Regression checks preserve **1,114** compiling indexed objects byte-for-byte,
including all **972** known exact results. Of **2,655** recent source/build
pairs, **2,565** retain identical objects, **89** retain their compilation
failures, and the one changed object contains the newly exact callback body.
All fourteen complete GX objects and the AX voice-parameter object remain
byte-identical. Compiler-library tests pass **1,600**, excluding the previously
confirmed embedded-assembly failure. Final recompilation verifies **195**
execution-panel and full-unit object hashes after the last test changes.

Artifacts are under `target/callback-spill-{canaries,previous-canaries,index,recent,library,full-ax}`.
Both canary directories contain fresh-reference, exactness, and execution
reports; `target/callback-spill-final-verification.json` records the final
compiler and object hashes. The earlier **1,024** GXMisc polling-spill
differences were still open at this checkpoint and are closed above. Broader
register allocation, instruction matching, and full-project parity remain
unfinished. These focused counts are not a corpus
parity estimate.

## Complete GXTexture translation unit and fourteen-unit GX coverage, 2026-09-07

BfBB's unchanged, fully configured `GXTexture.c` now compiles, advancing the
survey from **13/14 to 14/14 complete GX translation units**. Nine full texture
functions pass **9,216 three-way PowerPC execution comparisons** against a
fresh GC/1.2.5n object and the original DOL: `GXGetTexBufferSize`,
`__GetImageTileCount`, `GXInitTexObj`, `GXInitTexObjLOD`, `GXGetTexObjLODBias`,
`GXLoadTexObjPreLoaded`, `GXLoadTexObj`, `GXLoadTlut`, and
`GXInitTexCacheRegion`.

Inline output-pointer substitution now restores direct scalar writes to the
AST's assignment form. Definite-assignment and lifetime analysis can therefore
see the definitions made by grouped switch arms. Automatic scalar locals and
parameters qualify; static, volatile, array, aggregate, and global objects
retain memory stores. Narrow floating assignments to register locals use the
existing signed conversion packet and retain their declared width for later
promotion or truncation. Mixed arithmetic can promote an explicitly narrowed
integer through the shared integer-to-float conversion path.

Indirect-call arguments now participate in value substitution even when the
call is nested under a cast or assignment; the function-pointer target keeps
its snapshot identity. The global-record forwarding owner declines pointer
variables and non-leaf arguments. A single word-sized member argument can use
the shared indirect-call emitter with an allocator-owned callee temporary.
Small aggregate copies accept pointer sources and twelve-byte objects, copying
the first pair of words before the trailing word, including padding. New
pointer-source copies require word alignment and a matching retained element
size. These changes extend existing lowering owners rather than introducing
texture-specific implementations.

The full-function execution panel checks return bits, 1,024 bytes containing
texture objects and regions, the complete 1,456-byte GX context, ordered FIFO
writes, callback arguments, saved GPR/FPR images, and SP/LR/PC. Inputs include
all tile-format groups and defaults, zero and boundary dimensions, mipmap
termination, floating clamp boundaries, filter choices, and cache-region
sizes. The harness controls `memset`, `__GXFlushTextureState`, and texture/TLUT
region callbacks; these calls clobber volatile registers. `GXLoadTexObj`
executes the actual compiled `GXLoadTexObjPreLoaded` body. This is bounded
execution evidence, not a hardware or whole-project validation.

The pinned [GXTexture layout](reference-layouts/bfbb-gxtexture.json) verifies
**26/26 fresh-reference functions** byte-for-byte against the original DOL.
The candidate matches **2/26**, `GXGetTexObjFmt` and `GXGetTexObjTlut`, and has
**6,540 text bytes** versus **4,612** for the reference. Its `GXInitTexObj` jump
table differs from the pinned original image, leaving two address relocations
unresolved; all other measured relocations resolve. The DOL symbol reader now
accepts uniquely sized `.init` functions so the layout can verify `memset` as
an external runtime function. Register allocation, scheduling, frame layout,
and broader instruction matching remain open.

Canaries **1988–1997** cover output switches and mipmap loops, narrow floating
locals, mixed narrow arithmetic, callback member arguments, and pointer-based
aggregate copies. Candidate compilation improves **0/150 to 150/150 objects**
across fifteen builds at O0/O4; fresh reference compilation is **150/150**.
There are **22/150 whole-object matches** and **82/450 exact function text plus
symbolic relocation comparisons**. Of **230,400 paired execution comparisons**,
**229,888 agree**. The remaining **512 differences** are confined to
GC/1.1p1 O0 `member_callback` in canary 1995: the reference stores both pointer
parameters at SP+8, then uses the object's key as both the callee address and
its argument. These test inputs fault on that unmapped address. A separate
reference-specific model verifies the fault PC, link register, active frame,
register images, and absence of callback effects; the candidate does **not yet
reproduce** the overlapping spills. The differences remain an open parity gap,
not passing paired comparisons. The earlier **1,024** polling-spill differences
from the GXMisc checkpoint also remain open.

Regression checks retain **1,114** byte-identical compiling indexed objects,
including **972** known exact results. Of **2,505** recent source/build pairs,
**2,416** retain identical objects and **89** retain their compilation failures.
The thirteen previously compiling GX units and the AX voice-parameter unit
retain identical objects. Compiler-library tests pass **1,598**, excluding the
previously confirmed embedded-assembly failure; the DOL tool tests pass **26**.
Final recompilation verifies **166** execution-panel and full-unit object
hashes after the last code and test changes.

Artifacts are under `target/gx-texture-{canaries,real,index,recent,library,full-ax}`.
The canary directory records compilation, fresh-reference, exactness, and
execution results, retaining the **512** expected-but-unimplemented differences.
The real directory contains both objects, linked comparisons, and the nine
function execution report. `target/gx-texture-final-verification.json` records
the final compiler and object hash bridge. Complete GX compilation is a
milestone; full-project compilation and compiler parity remain open. These
focused counts are not a corpus parity estimate.

## Complete GXMisc translation unit and inline polling loops, 2026-09-07

BfBB's unchanged, fully configured `GXMisc.c` now compiles, advancing the GX
survey from **12/14 to 13/14 complete translation units**. `GXSetMisc`,
`__GXAbort`, and `GXDrawDone` pass **3,072 three-way PowerPC execution
comparisons** against a fresh GC/1.2.5n object and the original DOL. Checks
cover the complete 1,456-byte GX context, FIFO and PI writes, MEM-counter read
order, timer calls, interrupt tokens, queue/wakeup effects, saved GPR/FPR
images, and SP/LR/PC. Timer inputs exercise low-word wrap, signed 64-bit wrap,
negative elapsed values, exact timeout boundaries, and repeated polling.

Compound terminal switch arms now reach the shared structured CFG lowerer
before the older single-statement terminal owner rejects them. This includes
empty arms, guarded stores, and compound defaults. Inline composition admits
pre- and post-test polling loops, retaining its existing dominance and
argument-substitution checks. A post-test assignment's deferred interval now
extends through the condition after the body; recording that condition before
the body had incorrectly rejected locals first defined within a polling loop.

Retained wide frame values can place their comparison prelude at the end of
each post-test iteration. Arithmetic and frame allocation remain in the shared
backend. Narrow integer expressions execute at their original width before
sign- or zero-extension to a pair. Wide conditions with a continue targeting
the current loop, pre-test wide conditions, and wide initializer/step
expressions still decline; their prelude placement needs separate control-flow
support. These changes let the actual nested abort waits and draw-completion
waits compile without substituting host implementations for their loop bodies.

The native harness controls external services: `GXGetGPFifo`, `OSGetTime`,
`OSDisableInterrupts`, `OSRestoreInterrupts`, `OSSleepThread`,
`__GXSetDirtyState`, and `PPCSync`. Timer and counter sequences are identical
across the three objects; sleep calls update the queue and eventually set the
actual `DrawDone` global. `GXFlush` and the inline MEM-counter reader execute
their compiled bodies. Service calls clobber volatile registers so saved values
must survive actual calls. This is bounded execution evidence, not a hardware
or full-project validation.

The pinned [GXMisc layout](reference-layouts/bfbb-gxmisc.json) verifies **20/20
fresh-reference functions** byte-for-byte against the original DOL. The
candidate matches **1/20**, `GXReadDrawSync`, with every measured relocation
resolved. Its full object contains **2,176 text bytes**, versus **1,768** for
the reference. Register allocation, frame layout, scheduling, and broader
instruction matching remain open.

Canaries **1980–1987** add token-store switches, guarded/default/empty arms,
inline wide timer loops, nested counter-stabilization loops, and global
completion waits. Candidate compilation improves **0/120 to 120/120 objects**
across fifteen builds at O0/O4; fresh reference compilation is **120/120**.
There are **0/120 whole-object matches** and **28/240 exact function text plus
symbolic relocation comparisons**. Of **122,880 paired execution comparisons**,
**121,856 agree**. The remaining **1,024 differences** are confined to
GC/1.1p1 O0 canary 1985: its reference spills the input over saved r24 in
`wait_twice` and saved r26 in `guarded_settle` (**512 cases each**). Separate
reference-specific expected register images explain every discrepancy; the
candidate does **not yet reproduce** these two spill overlaps. They remain
open parity gaps, not passing paired comparisons.

Regression checks preserve **1,114** compiling indexed objects byte-for-byte,
including **972** known exact results. Of **2,385** recent source/build pairs,
**2,296** retain identical objects and **89** retain their compilation failures.
The twelve previously compiling GX units and the AX voice-parameter unit
retain identical objects. Compiler-library tests pass **1,595**, excluding the
previously confirmed embedded-assembly failure. Final recompilation verifies
all execution-tested canary and full-unit object hashes after formatting and
test additions.

Artifacts are under `target/gx-misc-{canaries,real,index,recent,library,full-ax}`.
The canary directory contains compilation/reference comparison results plus
`execution-results.json`, `polling-execution-results.json`, and
`global-polling-execution-results.json`; the polling report preserves the
**1,024** expected-but-unimplemented paired differences explicitly. The real
unit directory contains fresh objects, linked comparisons, and
`execution-results.json`. `target/gx-misc-verified-objects.json` records the
final compiler/object hash bridge. Full-project parity remains open, with
`GXTexture.c` the remaining GX compilation blocker. These focused counts are
not a corpus parity estimate.

## GC/1.1p1 O0 switch parameter spills, 2026-09-07

The **716 execution differences** exposed by the GXAttr canaries below are now
closed. GC/1.1p1 O0 `hinted` and `assigned` match their reference's complete
**120-byte** and **88-byte** function bodies and symbolic relocations,
including the wrong switch selector and corrupted saved r30.

The existing version-specific source-spill policy now admits a scalar switch
followed by a joined observer call. Parameters read once share SP+8 in source
order; an input read repeatedly instead occupies r30, whose saved image is
also SP+8. Emission uses the existing linkage-first prologue, typed expression
lowering, call emitter, and epilogue. The shared switch emitter accepts an arm
statement callback so this owner can assign already established local homes
without duplicating comparison trees or join patching. Recognition checks word
types, call signatures, supported expressions, and initialization on every
exit. Address-taken/effectful values, volatile locals, fallthrough, and other
control-flow shapes retain their existing owners. This is a measured family
within the spill bug, not a complete model of the original register allocator.

Canaries **1978–1979** add constant-valued and input-valued switches, initialized
results, signed selectors, different constants, and no discarded-value hints.
Candidate and fresh reference compile **30/30 objects** across fifteen builds
at O0/O4. **10/120 function text plus symbolic relocation comparisons** match,
including the new GC/1.1p1 O0 `assigned_choices` and `signed_choices`; **0/30
whole objects** match. The earlier 120-object panel retains full compilation
and **0/120 whole-object matches**, with exact functions increasing **4/450 to
6/450**. The two panels add four exact function comparisons over the previous
compiler.

All **291,840 paired PowerPC execution comparisons** agree: **230,400** in
canaries 1970–1977 and **61,440** in the new panel. Independent expected models
include the reference spill bugs in **1,991 cases**. Checks cover observer
arguments, ordinary memory effects, saved GPR/FPR images, and SP/LR/PC; helper
calls clobber volatile registers. The new panel also verifies 60 bytes of the
caller frame and permits the ABI linkage word to remain unchanged or hold LR.
Three newer reference builds tail-call the observer at O4, whereas the
candidate saves LR and calls it normally; those linkage writes and instruction
schedules remain different. These are behavior matches under the stated
observations, not whole-object matches or ABI-correctness claims for the buggy
reference cases.

Regression checks retain **1,114** byte-identical compiling indexed objects,
including **972** known exact results. All **2,146** compiling recent objects
remain identical and **89** retain their compilation failures. The **12**
compiling full GX units and the AX voice-parameter unit retain identical
objects. Compiler-library tests pass **1,592**, excluding the previously
confirmed embedded-assembly failure. Final recompilation verifies the hashes
of all **150** execution-tested canary objects and **13** complete library
objects after the last refactor.

Artifacts are under `target/switch-spill-{canaries,prior-canaries,index,recent,library,full-ax}`,
including `reference-results.json`, `reference-comparison.json`, and
`execution-results.json` for both canary panels. The final compiler/object
bridge is `target/switch-spill-verified-objects.json`. Full-project parity
remains open; the GX compilation frontier remains **12/14**, with `GXMisc.c`
and `GXTexture.c` still blocked. These focused counts are not a corpus parity
estimate.

## Complete GXAttr translation unit and switch-table liveness, 2026-09-07

BfBB's unchanged, fully configured `GXAttr.c` now compiles, advancing the GX
survey from **11/14 to 12/14 complete translation units**. Its
`__GXCalculateVLim`, `GXSetVtxAttrFmtv`, and `GXSetTexCoordGen2` pass **3,072
three-way PowerPC execution comparisons** against a fresh GC/1.2.5n object and
the original DOL. Checks cover the complete 1,456-byte GX context, format-list
memory, FIFO writes, saved GPR/FPR images, and SP/LR/PC. The only controlled
helper is `__GXSetMatrixIndex`; its argument, context effects, and volatile
register clobbers are checked. The real inline format dispatcher executes in
all three objects.

Computed integer arithmetic now uses independent virtual operands after the
specialized schedules decline, including signed-byte promotion and variable
accesses to small SDA byte arrays. Uninitialized scalar declarations used only
as bare discarded-value hints no longer request register homes. Initialized,
assigned, address-taken, volatile, and assembly-visible locals retain their
existing handling. Frames establish unnamed saved slots reserved for loop
pressure, and leaf linkage removal accepts intervening saved-register reloads
without deleting those reloads.

The allocator now includes switch-table successors in liveness. A separate
module recovers the selected dispatcher's table identity from relocations and
register copies, allowing hoisted table addresses and distinct dispatches.
This recovery follows the emitted instruction stream; it is not general
indirect-branch analysis. Dense switches use virtual temporaries instead of
rewriting named parameter homes inside a loop, preserving the first loop test
and values used by switch arms. Condition cleanup uses the shared instruction
retargeting helper so branch removal also adjusts jump-table entries. Native
execution exposed both the missing indirect edges and stale table offsets.

Canaries **1970–1977** cover arithmetic/casts/indexed loads, the vertex-limit
accumulator, inline format lists, and discarded local hints. Across fifteen
builds at O0/O4, compilation improves **30/120 to 120/120 objects**; the fresh
references also compile **120/120**. There are **0/120 whole-object matches**
and **4/450 exact function text plus symbolic relocation comparisons**.
Of **230,400 paired execution comparisons**, **229,684 agree**. Candidate output
passes the independent C/ABI models throughout this panel. The remaining
**716 differences** expose two further GC/1.1p1 O0 reference spill bugs in
canary 1977: `hinted` reads the second parameter from the shared spill slot as
its switch selector (**204 cases**), while `assigned` restores the first
parameter over saved r30 (**512 cases**). A separate **1,536-case** rerun
validates these reference-specific models with no unexplained discrepancies;
the candidate does **not yet reproduce** those two bugs. These are open parity
gaps, not passing paired comparisons.

The pinned [GXAttr layout](reference-layouts/bfbb-gxattr.json) verifies five
fresh-reference functions byte-for-byte against the original DOL. The linked
checker now accepts verified initialized `.sdata` images and disambiguates
reused source ordinals by their pinned address and size, while retaining
unique-image checks. The candidate matches **0/5** linked functions. Its full
object has **4,264 text bytes**, versus **3,412** for the reference; frame layout,
scheduling, and jump-table images remain different. Matching execution of the
three tested functions does not establish full GXAttr or project parity.

Regression checks preserve **1,114** compiling indexed objects byte-for-byte,
including **972** known exact results. Of **2,235** recent source/build pairs,
**2,042** retain identical objects, **89** retain their compilation failures,
and **104** matrix objects change with the corrected liveness. The complete
120-object matrix panel passes **107,520** execution checks against independent
models and existing original-game fixtures. Final recompilation verifies all
120 execution-tested hashes. Ten other complete GX units and the AX voice
parameter unit retain identical objects; changed `GXPerf.c` passes **4,701
three-way execution comparisons** against a fresh reference and the DOL.
Compiler-library tests pass **1,590**, excluding the previously confirmed
embedded-assembly failure; allocator tests pass **104** with **8 ignored**, and
all **21** linked-checker tests pass.

Artifacts are under `target/gx-vlim-{canaries,real,perf,matrix,index,recent,library,full-ax}`.
The canary directory records compilation, reference comparison, execution, and
`hint-spill-bug-execution-results.json`; real-unit directories record compiled
objects and native execution. `target/gx-vlim-verified-objects.json` records the
matrix hash bridge. These focused counts are not a corpus parity estimate.
`GXMisc.c` and `GXTexture.c` remain the two GX compilation blockers, alongside
the new O0 spill-bug reproductions and instruction matching work.

## GC/1.1p1 O0 transaction and context spill bugs, 2026-09-07

The **1,365 candidate/reference execution differences** exposed by the GXFifo
milestone below are now closed. The existing GC/1.1p1 O0 bug profile admits two
more scalar transaction families and a guarded inline context image. Other
builds and optimization levels retain their existing lowering.

The transaction emitter deliberately spills the source parameter over saved
r30 at SP+8. Its epilogue restores that overwritten value. Global snapshot and
computed-store transactions use the existing prologue, call, global access,
and epilogue machinery. Recognition checks parameter/local types and call
signatures before admitting the fixed register schedule. The context case
instead expresses the prior-pointer spill as the first word of the aggregate
image before ordinary frame allocation. Calls that overwrite that image also
overwrite the pointer subsequently passed to the restore service. Recognition
uses the typed statement shape rather than source symbol names or a fixed
aggregate size; the normalization is idempotent.

Canaries **1966–1969** add integer and callback-pointer swaps, signed/unsigned
computed stores with different arithmetic constants, and a **32-byte** context
image alongside the existing **712-byte** image. Candidate and fresh reference
compile **60/60 objects** across fifteen builds at O0/O4. **1/60 whole objects**
and **4/180 function text plus symbolic relocation comparisons** are exact:
all four transactions in GC/1.1p1 O0 canary 1967. Rechecking canaries 1956–1965
retains **150/150** compilation and **45/150** whole-object matches, while exact
functions increase **148/390 to 150/390**: `nested` and `protected_swap` now
match their reference's complete 80-byte bodies and relocations.

All **291,840 paired execution comparisons** agree: **199,680** from the prior
cohort, **61,440** new transaction cases, and **30,720** small-context cases.
Independent expected models include the measured reference bugs in **3,754**
cases; these are behavior matches, not claims of C/ABI correctness. Checks cover
helper arguments, memory/global effects, callback replacement between the
condition and call, overwritten context images, return values, saved GPR/FPR
images, and SP/LR/PC. The small-context callback writes within the smaller
image. The two context `dispatch` bodies now have the same **124-byte** size as
the reference but remain instruction-inexact: the ordinary allocator places
the aliased image at SP+16 rather than the reference's SP+8. Exact frame layout
and broader shared-spill scheduling remain open.

Focused regression checks preserve **1,114/1,114** compiling indexed objects,
including **972** known exact results. Of **2,025** recent source/build pairs,
**1,936** retain identical objects and **89** retain their compilation failures;
this panel includes the earlier shared-parameter-spill family. All **eleven**
compiling full GX translation units and the full AX voice-parameter unit retain
identical objects. Compiler-library tests pass **1,589**, excluding the known,
previously baseline-confirmed embedded-assembly test; all **18** linked-checker
tests pass. Final recompilation verifies all **210** execution-tested canary
objects remain byte-identical after formatting and recognition guards.

Measurement artifacts are under `target/spill-transactions-{prior-canaries,canaries}`
(`reference-results.json`, `reference-comparison.json`, `execution-results.json`,
and the new cohort's `context-execution-results.json`), with indexed/recent
regressions under `target/spill-transactions-{index,recent}` and full units under
`target/spill-transactions-{library,full-ax}`. The final object hash bridge is
`target/spill-transactions-verified-objects.json`. These focused counts are not
a corpus parity estimate. Full-project parity remains open; the GX compilation
frontier remains **11/14**.

## Complete GXFifo translation unit and retained context frames, 2026-09-07

BfBB's unchanged, fully configured `GXFifo.c` now compiles, advancing the GX
survey from **10/14 to 11/14 complete translation units**. Three entry points
pass **3,072 three-way native execution comparisons** against the original DOL
and a fresh reference object: `GXCPInterruptHandler`, `GXGetFifoPtrs`, and
`GXSetBreakPtCallback`. Full-project parity remains open, including three newly
exposed GC/1.1p1 O0 shared-spill cases described below.

Retained void inline bodies now admit uninitialized automatic aggregate images
as frame storage, just as fixed arrays already did. Each inline instance keeps
its own renamed declaration; scalar uninitialized-read checks remain active.
Single-iteration `do { ... } while (0)` macro blocks compose recursively when
their statements are eligible, preserving the existing rejection of unsupported
control transfers. A frame-backed structured trial can handle a guarded callback
that is read again after intervening calls, before the conservative global-reuse
diagnostic rejects the function. The exception-context path uses the ordinary
frame planner, allocator, and callback emitter.

The paired-load arithmetic owner recognizes pointer-member subtraction through
byte-pointer casts. Wider-pointee casts keep their scaling semantics. Global
pointer initializers retain their captured values when calls or writes may
replace the source global; direct named store destinations now count as writes
in this copy-propagation check. This fixes a callback setter that previously
rewrote `return old_callback` into a fresh read of the replacement global.

Execution of `GXGetFifoPtrs` exposed a separate existing high-half addition
miscompile. `addis` with RA=0 reads literal zero, even if an earlier instruction
put the operand in r0. The constant arithmetic emitter now places that operand
in a nonzero register while allowing the result to use r0. The reference's
masked physical-address conversion and the candidate now produce the same
pointer. A separate baseline panel confirms **630/630 incorrect results** from
the old emitter across thirty compiling high-half-store objects; the fresh
reference passes the same model cases.

Canaries **1956–1965** cover inline aggregate contexts, repeated inline instances,
macro blocks, byte-pointer member differences, global pointer snapshots, and
computed high-half stores. Baseline compilation is **30/150** complete objects;
candidate and fresh reference compilation is **150/150** across fifteen builds
at O0/O4. **45/150 whole objects** and **148/390 function text plus symbolic
relocation comparisons** are exact. Both byte-pointer cohorts are wholly exact;
the O4 high-half-store cohort contributes the other fifteen exact objects.

All **199,680 candidate executions** satisfy the source/memory/ABI model. Of the
paired candidate/reference executions, **198,315 agree** and **1,365 differ**.
Every difference is in **GC/1.1p1 O0**, extending coverage of the existing
`SharedUnoptimizedParameterSpills` bug beyond its implemented owner:

- In `1957`'s `dispatch`, the reference spills the prior-context parameter at
  the start of the inline aggregate. The second clear overwrites that parameter;
  **341 cases** pass the overwritten word to `set_context`. The candidate retains
  the original pointer. The raw reference instructions place both at SP+8.
- In `1959`'s `nested`, the reference restores r30 from storage overwritten by
  the output-pointer parameter: **512 saved-register differences**.
- In `1963`'s `protected_swap`, the reference restores r30 from storage overwritten
  by the replacement pointer: **512 saved-register differences**.

These samples remain in the corpus. They are **unmatched reference bugs**, not
passing parity results or filtered invalid inputs. The other fourteen builds
and the remaining GC/1.1p1 cases agree. Callback services deliberately change
the callback global between the condition and the indirect call; snapshot
services change their global during entry/exit calls, proving that captured
values and fresh reads remain distinct.

The real FIFO panel varies interrupt enable/status combinations, callbacks,
prior overflow state/count, CPU/GP FIFO membership, hardware register images,
FIFO contents, and overlapping output-pointer destinations. It compares all
**1,456 GX context bytes**, **1,024 object/output bytes**, hardware-register
memory and ordered access traces, callback/context events, persistent globals,
and saved GPR/FPR/SP/LR state. Six external OS services and two callback targets
are controlled identically, including volatile-register clobbering. The two
in-unit FIFO interrupt helpers execute their actual candidate/reference/DOL
bodies. This validates the selected entry points and those helpers, without
claiming execution of the controlled OS service bodies.

The complete candidate object is **6,344 ELF bytes / 2,336 text bytes**, SHA-256
`2122482415e4adc02d253cab38233b4910425d0ea249aa080f429ededb344c57`.
The fresh reference is **5,728 ELF bytes / 2,044 text bytes**, SHA-256
`d218540f9c38d0b3d1fb2a80265c54c35da02550a8e53954653e6673cdb0c06b`.
The pinned `docs/reference-layouts/bfbb-gxfifo.json` checks the three entry
points and `__GXWriteFifoIntEnable`/`__GXWriteFifoIntReset`: **0/5 candidate**
and **5/5 fresh reference** exact linked functions, with no unresolved relocations.

Regression panels preserve **1,098** prior indexed objects, including **972 known
exact matches**, and add **16 compiling pairs** among **1,674**. Those sixteen
pairs (canary 993 on all builds and 1272 on GC/1.2.5n) pass **3,968** additional
reference execution comparisons, including changes to the global state pointer
across calls. All **1,936** previously compiling recent objects are unchanged
among **2,025** pairs; **89** still decline. The previous ten GX objects and
complete AXVPB remain byte-identical. **1,589 compiler library tests pass** with
one known embedded-assembly test excluded; that test was separately rerun and
still fails. **18 linked-checker tests pass**. Final recompilation preserves all
150 new candidate objects and the real FIFO object byte for byte.

Together the candidate/reference panels check **206,720 executions**: **205,355
match** and the **1,365 shared-spill differences remain open**. Next GX frontiers
are GXAttr's retained descriptor helper, GXMisc's switch-arm scheduling, and
GXTexture's fallthrough switch. Exact FIFO schedules, broader shared-spill bug
reproduction, and full project builds remain unfinished.

Artifacts: `target/inline-context-canaries/{results,reference-results,
reference-comparison,execution-results,baseline-high-execution-results}.json`,
`target/inline-context-real/{compilation-results,execution-results,fixtures,
candidate-verified-linked-text,reference-verified-linked-text}.json`,
`target/inline-context-{index,recent,library,full-ax,new-controls}/`,
`target/inline-context-{tests-final,known-failure,linked-tests,execution,
real-execution,new-controls}.log`, and
`target/inline-context-preformat-objects.json`.

## Complete GXInit translation unit, 2026-09-07

BfBB's unchanged, fully configured `GXInit.c` now compiles. The GX survey
advances from **9/14 to 10/14 complete translation units**. `GXInit` itself
passes **512 three-way execution comparisons** against a fresh reference
object and the original game DOL. Its candidate instructions remain non-exact:
**2,072 bytes versus 1,936**. The fresh reference's linked instructions are
**exactly equal to the original DOL**, with no unresolved relocations on either
side using `docs/reference-layouts/bfbb-gxinit.json`.

Three shared lowering changes remove the unit's blockers:

- Structured-body normalization converts assignments to known static/global
  storage into ordinary memory stores before local liveness and allocation.
  Local and parameter bindings retain precedence over global names. Existing
  statement traversal covers nested branches, loops, and switch bodies.
- A typed absolute-address constant folder preserves the pointee stride through
  addition/subtraction and pointer casts, including null addresses and wrapping
  32-bit addresses. Integer casts end pointer scaling. This resolves the SDK's
  uncached-to-physical address expression without asking for a register leaf
  for a large literal. Side-effecting expressions remain with normal emission.
- Computed member-array stores accept pure computed indices and pure intrinsic
  values. The existing address-retention path keeps the aggregate base alive
  while computing the index and completes the destination address before
  evaluating the stored value. Earlier exact schedules retain first refusal.

Canaries **1950–1955** cover static registration, persistent state updated in a
loop, local/parameter shadows, thirteen constant-address forms, and four
computed-index stores. Across O0/O4 and fifteen builds, the frozen preceding
behavior compiles **0/90** complete objects; candidate and fresh references
compile **90/90**. **23/90 whole objects** and **395/630 function text plus
symbolic relocation comparisons** are exact. All **322,560 candidate/reference
execution comparisons** pass, checking returns, complete memory images,
persistent objects, ordered calls/arguments, saved GPR/FPR images, SP/LR, and
execution faults. The address cohort accounts for all whole-object matches;
computed-index schedules remain non-exact.

The real `GXInit` panel varies prior reset registration, bus-clock values,
HID2, FIFO base/size, and randomized initial context/register images. It compares
all **1,456 context bytes**, the **128-byte FIFO object**, hardware pointer
assignments, reset-registration state, ordered FIFO writes, service-call
arguments, and the ABI. Sixteen services are controlled identically on all
three machines, including `__GXInitGX`, FIFO/texture/TLUT initialization, reset
registration, and the PPC accessors. Destination services write deterministic
images; texture flushing changes the bus-clock memory after the initial read;
services clobber volatile registers. This tests `GXInit`'s own control flow,
retained values, calls, and stores, without claiming execution coverage of those
services' bodies. Combined panels have **323,072 passing comparisons**.

The complete candidate object is **15,536 ELF bytes / 5,180 text bytes**, SHA-256
`9ec0166a974c56c8a35376ea04a73fa1fe486494fb5d32f687f90836ecf9d7eb`.
The fresh complete reference is **14,600 ELF bytes / 4,996 text bytes**, SHA-256
`40890b2d0fac6e42ba13c34ae9efe5809515b77a2e323cf93c1a3f301d8a116e`.
The linked comparison selects `GXInit`; it does not claim exactness of the
complete object. The source files and project configuration were not modified.

The linked-DOL checker now verifies anonymous BSS anchors against an explicitly
named, equally sized object in the matching NOBITS section. Each relocation
addend must stay within that verified object's extent. It also accepts a pinned
literal symbol/range to verify the SDA section when the r2/r13 address windows
overlap. Anonymous label ordinals alone do not establish a placement. **18
checker tests pass**, including rejection of mismatched storage, missing or
conflicting anchors, and addends outside the verified object.

Focused regressions preserve all **1,098** previously compiling indexed objects,
including **972 known exact matches**, among **1,674** pairs. All **1,846**
previously compiling recent objects are unchanged among **1,935** pairs; the
remaining **89** still decline. The prior nine GX objects and complete AXVPB
object remain byte-identical. **1,588 compiler library tests pass**, excluding
the previously baseline-confirmed
`inline_expansion::tests::composes_zero_argument_embedded_asm_at_a_nested_call_site`
failure. Remaining GX frontiers are GXFifo's retained callback helper, GXAttr's
retained descriptor helper, GXMisc's switch-arm scheduling, and GXTexture's
fallthrough switch. Full project builds and exact initialization schedules
remain open.

Artifacts: `target/gx-init-canaries/{results,reference-results,
reference-comparison,execution-results}.json`,
`target/gx-init-real/{compilation-results,execution-results,fixtures,
candidate-verified-linked-text,reference-verified-linked-text}.json`,
`target/gx-init-{index,recent,library,full-ax}/`, and
`target/gx-init-{tests-final,linked-tests,execution,real-execution}.log`.

## GX shutdown and retained 64-bit frame values, 2026-09-07

BfBB's unchanged `__GXShutdown` function now compiles and passes **2,048
original-DOL execution comparisons** on both the candidate and a fresh reference
object. The full `GXInit.c` source advances to a later call-survivor diagnostic
in `GXInit` itself. **GX compilation remains 9/14** complete translation units.

Wide call-result stores capture the complete EABI r3:r4 result. Global and
pointer destinations receive the low word at offset four and the high word at
offset zero; the existing saved-pointer call owner uses the same two-word
semantics. The general store path reserves both result registers while computing
a pure destination address and declines side-effecting address evaluation.
It excludes local bindings from global-symbol recognition.

A new `wide_frame_values` fallback represents retained 64-bit automatic values
as aligned, eight-byte frame images. Existing specialized pair emitters retain
first refusal through a cloned trial, preserving their measured schedules.
The fallback reuses the ordinary frame allocator, call emitter, and structured
control flow. It lowers entry initializers in declaration order and captures
both words before overwriting a pair destination. Addition/subtraction carry
and borrow, signed/unsigned comparisons, equality, and explicit low-word
conversions become scalar operations with explicit temporary values. This also
keeps timestamps intact across intervening calls. Static-local assignments use
ordinary global stores; the original object pipeline retains their storage.

The frame fallback is deliberately a separate implementation choice from
register-pair scheduling. Address escapes, volatile wide operands, wide
parameters/returns, and wide loop-condition preludes remain outside this lane.
Unsupported short-circuit expressions decline before any pair reads are hoisted;
a focused unit test checks that boundary. These restrictions leave existing
specialized handlers and diagnostics available.

Canaries **1946–1949** cover two complete-result stores and six frame/value
functions at O0/O4 across fifteen builds. The frozen `0f73ee0c` baseline compiles
**0/60** complete objects; candidate and fresh references compile **60/60**.
**21/60 whole objects** and **54/240 function text plus symbolic relocation
comparisons** are exact. All exact functions are in the call-store cohort;
the frame-backed schedules remain non-exact. All **245,760 candidate/reference
executions** agree on returns, complete global/output memory, call order and
arguments, saved GPR/FPR images, SP/LR, and execution faults. Inputs cover
sign boundaries, low-word carry/borrow, near-threshold differences, equality,
and the wrapping arithmetic observed in the reference output. An ordered
initializer control retains both a scalar token and the wide result across a
later call that changes the comparison state.

The shutdown probe uses the actual project headers, the preceding write-gather
assembly helper, and the unchanged shutdown body; a function-pointer initializer
keeps the static function emitted. It checks all 1,456 GX context bytes, the
command-processor register image, three persistent shutdown objects, ordered
hardware reads/writes, external call order/arguments, and the ABI. Cases vary
final/nonfinal paths, prior initialization, timer thresholds, counter equality,
halfword rollover, and zero through four readback retries. Its six external
services are controlled identically: `OSGetTime` returns supplied 64-bit values;
three callback setters and `__GXAbort` record calls and clobber volatile registers;
`PPCSync` records the barrier-service call without executing its OS trap. This
validates shutdown control flow and call behavior, not those services' bodies.
Together the panels have **247,808 passing candidate/reference comparisons**.

The extracted candidate is **2,144 ELF bytes / 520 text bytes**, SHA-256
`2e3048b9420128a481d8df7e3f61db1eb5f322cfdaedbe52830c2ca309996e8c`.
The fresh reference is **1,984 ELF bytes / 412 text bytes**, SHA-256
`62f200ba4386a9788c8159b97398319cf3c433845061a7ecb11eaa47557671bf`.
`__GXShutdown` itself is **508 versus 400 bytes**. The pinned
`docs/reference-layouts/bfbb-gxshutdown.json` reports **0/1 candidate** and
**1/1 reference** exact linked functions, with no unresolved relocations.
The companion write-gather helper accounts for twelve text bytes on each side.

The linked-DOL checker accepts explicit BSS symbol aliases for source-local
objects whose numeric suffixes change when a function is extracted. It verifies
the original symbol/address, DOL BSS range, candidate object type, matching
section/size, and section bounds, and rejects missing/conflicting aliases.
**15 checker tests pass**. The three shutdown aliases connect the extracted
`peCount$18`, `time$19`, and `calledOnce$20` objects to the original `$35/$36/$37`
objects; no guessed placement or unchecked relocation is treated as exact.

Regression controls retain **300/300** exact memory objects; **1,098** unchanged
indexed objects, including **972** known exact results and **576** identical
declines; and cumulative metadata's **1,494** compiled, **1,486** unchanged,
**1,179** known exact, and **704** identical declines. Metadata's eight changes
predate this checkpoint. All **1,786** compiling recent objects are unchanged,
with **89** identical declines among **1,875** pairs. A separate **120-pair** wide
control panel retains **51** unchanged compiling objects and **69** identical
declines. The prior nine GX objects and full AXVPB are byte-identical.
**1,587 compiler library tests pass**, excluding the previously baseline-confirmed
`inline_expansion::tests::composes_zero_argument_embedded_asm_at_a_nested_call_site`
failure. Final recompilation preserves all sixty new candidate objects and the
shutdown object byte for byte. Full-project builds, the remaining five GX units,
optimized wide schedules, broader wide expression forms, and further reference
bugs remain open.

Artifacts: `target/wide-clock-canaries/{results,reference-results,
reference-comparison,execution-results}.json`,
`target/wide-clock-shutdown/{shutdown.c,compilation-results.json,
execution-results.json,fixtures.json,candidate-verified-linked-text.json,
reference-verified-linked-text.json}`,
`target/wide-clock-{index,metadata,recent,wide-regression,gx-library,full-ax}/`,
`target/wide-clock-{tests-final,linked-tests,execution,shutdown-execution}.log`,
and `target/wide-clock-preformat-objects.json`.

## GXInit assembly and legacy counter-readback behavior, 2026-09-07

The unmodified BfBB `GXInit.c` advances past its write-gather assembly helper
and retained inline memory-counter reader. Its next diagnostic is the 64-bit
expression in `__GXShutdown`. **GX compilation remains 9/14** translation
units; this checkpoint advances a real unit's blockers without claiming that
the complete unit now compiles.

The assembler maps `andi.` and `andis.` to the existing record-form machine
instructions. Canaries **1939/1940** cover compact `andi.r3` tokenization,
zero/full/high-bit masks, distinct destination registers, condition-register
reads, and conditional branches. Candidate and fresh references produce
**30/30 byte-identical objects** across fifteen builds at O0/O4, including all
**240 function text plus symbolic relocation comparisons**. All **122,880
candidate/reference executions** agree with the result and condition-flag model.
The harness seeds SO with an actual `mtxer` instruction, checks CR0 including
SO, and verifies that unrelated condition fields and saved registers survive.

Retained integer inline helpers can now compose read-only memory-sampling loops
through the existing statement lane. Post-test dominance checks visit the body
before the condition and retain assignments that are available after its first
iteration. Locals remain hygienically renamed; scalar arguments are captured
per invocation. Calls, escaping locals, and nonlocal control flow retain their
existing eligibility checks. Unit tests distinguish a post-test assignment from
the same uninitialized read in a pretest condition.

Fresh reference execution exposed a **2.3.3 O4 compiler bug**: GC/1.1, GC/1.1p1,
GC/1.2.5, and GC/1.2.5n move a split-counter loop's second high-half read ahead
of its low-half read, including volatile accesses. The source's initial
`high, low, high` read sequence becomes `high, high, low`. The same builds at
O0–O3 and the later builds preserve source order in this panel. A build-profile
policy resolves into an O4-only `BugReproduction` quirk. The isolated
`legacy_readback_schedule` transform applies the measured halfword schedule
through ordinary statement emission. It checks the snapshot/comparison shape,
common global pointer base, and address dependencies, excludes shadowed globals,
and is idempotent. Semantic inline composition remains independent of this bug.

Canaries **1941–1945** cover fixed/dynamic register indices, repeated calls with
an intervening output store, and guarded calls at O0–O4 across fifteen builds.
**153,600 candidate/reference comparisons agree**, including exact read
addresses, widths, values, returns, output memory, saved GPR/FPR images, SP/LR,
and execution faults. Inputs cover zero through four retries and halfword
rollover. **7,168 cases differ from the independent source-order model on both
sides**; these are deliberately reproduced reference results, not semantic
correctness passes. Instruction parity remains **0/75 whole objects** and
**0/300 function comparisons** for these counter samples. Together, the seven
new canaries advance from **0/105** compiling baseline objects at `b0638235` to
**105/105** candidate and reference objects, with **30/105** whole objects exact.

The O1–O3 controls reset optimization with `-O0` before selecting their level.
The reference retains O4 scheduling when only a lower optimization flag is
appended after `-O4`; parity for that cumulative option sequence remains open.
The separate 25-pair optimization probe sets each level directly.

A probe using the **actual BfBB SDK headers** and the unchanged
`IsWriteGatherBufferEmpty` source body compiles with the project's configured
GC/1.2.5n flags. Its write-gather helper is **12/12 instruction bytes exact**
against the fresh reference object. Wrappers around `__GXReadMEMCounterU32`
and `__GXReadPECounterU32` pass another **1,024 execution comparisons**, all
reproducing the original read-order difference. The probe is **952 candidate
versus 856 reference ELF bytes**, SHA-256
`c1d7d08e00c8705dbbf58487124eda1dd1c8c196780b58fffcbb8f16789eb04a` and
`62b86eb5a2ccb1656d26e084523bd95131ca1d010ec0eac1c8445ad97ccd27f5`, respectively.
Its two counter bodies still differ in instruction scheduling and size. These
are SDK-source/reference-object checks; no whole GXInit or linked-DOL parity
claim is made. Overall, **277,504 candidate/reference executions agree**,
including **8,192 reproduced source-order-model differences**.

Regression controls retain **300/300** exact memory objects; **1,098** unchanged
indexed objects, including **972** known exact results and **576** identical
declines; and cumulative metadata's **1,494** compiled, **1,486** unchanged,
**1,179** known exact, and **704** identical declines. Metadata's eight changes
predate this checkpoint. All **1,681** compiling recent objects are unchanged,
with **89** identical declines among **1,770** pairs. The nine previously
compiling GX objects and full AXVPB are byte-identical. **1,586 compiler library
and 48 version-policy tests pass**, excluding the previously baseline-confirmed
`inline_expansion::tests::composes_zero_argument_embedded_asm_at_a_nested_call_site`
failure. Full-project builds, the remaining five GX units, counter instruction
schedules, cumulative optimization flags, and further reference bugs remain open.

Artifacts: `target/asm-mask-canaries/{results,reference-results,
reference-comparison,execution-results}.json`,
`target/asm-mask-sdk/{results,function-comparison,execution-results}.json`
and its generated `sdk.c`,
`target/asm-mask-optimization/results.json` and its disassemblies,
`target/asm-mask-{index,metadata,recent,gx-library,full-ax}/`,
`target/asm-mask-{tests-final,version-tests-final,execution-final,
sdk-execution-final,gx-library-final}.log`.

## Full GXGeometry and preserved loop/local values, 2026-09-07

The complete, unmodified BfBB `GXGeometry.c` compiles with its configured
GC/1.2.5n flags. **GX compilation advances from 8/14 to 9/14** translation
units. All ten functions pass **5,120 original-DOL execution comparisons** on
the candidate and a fresh reference object. `__GXSetDirtyState` and
`__GXSendFlushPrim` also match the linked original exactly, at 128 and 136 bytes.

Loop-carried local analysis now includes binding writes in a for-loop's step,
including postfix updates and assignments nested in comma expressions. The
fallback remains restricted to leaf functions; existing specialized schedules
retain first refusal. The fixed-port zero-fill matcher accepts the SDK macro's
redundant unsigned-halfword conversion after proving the source type. This
recovers the original flush schedule through its existing emitter.

Two plain member operands can share a nonvolatile global pointer through the
existing scoped cache. The cache ends with the expression, allowing subsequent
stores to change the pointer. Computed integer stores into member arrays reuse
the existing address-preservation path, keeping the target address live while
the value uses scratch registers. Constants retain their prior owner. Local
switch scrutinees and returned snapshots now use the shared leaf emitter when
intervening stores prevent substitution. The store scheduling guard recognizes
a final read of the immediately preceding store's target as a dependency.

Canaries **1931–1938** contain fifteen functions at O0/O4 across fifteen builds.
The frozen `667b2771` baseline compiles **0/120** complete objects; candidate
and fresh references compile **120/120**. All **230,400 candidate calls** and
corresponding reference calls pass the independent memory, FIFO, return, and
ABI checks. Controls cover continued loops, postfix steps, pointer replacement,
repeated indexed bitfield writes, reuse of an index after a store, constant
stores, and output pointers aliasing a switch's source. Instruction parity is
still open: **0/120 whole objects** and **0/450 function text plus symbolic
relocation comparisons** are exact.

The full GXGeometry panel checks all 1,456 context bytes, output memory,
ordered hardware-write addresses/widths/values, GPR14–31, FPR14–31, SP, LR, and
execution faults. Each function has 512 cases spanning dirty-state combinations,
legal geometry arguments, zero/nonzero flush extents, and randomized surrounding
state. Five external helpers (`__GXSetSUTexRegs`, `__GXUpdateBPMask`,
`__GXSetVCD`, `__GXSetVAT`, and `__GXCalculateVLim`) are controlled identically
on each side: they record calls, mutate dirty state, and clobber volatile
registers. Internal calls execute the actual unit code. These comparisons
validate the unit and its call behavior, not those five helpers' full bodies.

Full GXGeometry is **2,776 ELF bytes / 1,064 text bytes**, SHA-256
`86a7d5eff8afed8c481ecd5698c8f406de662d645522ab67d360c38d27897f7d`.
The fresh reference is **2,352 ELF bytes / 896 text bytes**, SHA-256
`c075c2bc3d769a5e372048928a03bc1d8487d1023683e93f2d48c1ee0a0ff3e5`.
The pinned `docs/reference-layouts/bfbb-gxgeometry.json` reports **2/10 candidate**
and **10/10 reference** exact linked functions, with no unresolved relocations.
The remaining eight functions have instruction-scheduling/size gaps.

Regression controls retain **300/300** exact memory objects. The indexed panel
has **1,098** compiled, **1,096** unchanged, **972** retained exact results, and
**576** identical declines. Newly compiling GC/1.2.5n canary **1275** passes
**4,096 candidate/reference execution comparisons**, covering every unsigned-byte
first argument, all eight indices, and varied full-byte second arguments without
assuming booleans. Together these panels total **239,616 passing candidate calls**.
The sole changed previously compiling indexed object, GC/1.2.5 canary **1313**,
differs only in `.strtab`; every function's text and symbolic relocations remain
identical. Cumulative metadata has **1,494** compiled, **1,486** unchanged,
**1,179** retained exact results, and **704** identical declines; its other seven
changes are historical `1469` results. All **1,561** compiling recent objects are
unchanged, with **89** identical declines among **1,650** pairs. The prior eight
GX objects and full AXVPB are byte-identical.

**1,583 compiler library tests pass**, excluding the previously baseline-confirmed
`inline_expansion::tests::composes_zero_argument_embedded_asm_at_a_nested_call_site`
failure. Formatting preserves all 120 new candidate objects and full GXGeometry
byte for byte. Full-project builds, five configured GX units, remaining
instruction schedules, and further reference-bug reproduction remain open.

Artifacts: `target/geometry-loop-canaries/{results,reference-results,
reference-comparison,execution-results}.json`,
`target/geometry-loop-full/{compilation-results,full-execution-results,
candidate-verified-linked-text,reference-verified-linked-text}.json`, its ten
DOL fixtures, `target/geometry-loop-index/{1275-execution-results,
1313-baseline-comparison,1313-reference-comparison}.json`,
`target/geometry-loop-{index,metadata,recent,gx-library,full-ax}/`, and
`target/geometry-loop-{tests-final,execution-final,full-execution-final}.log`.

## Full GXPerf and casted computed-pointer accesses, 2026-09-07

The complete, unmodified BfBB `GXPerf.c` compiles with its configured
GC/1.2.5n flags. **GX compilation advances from 7/14 to 8/14** translation
units. Both functions pass **4,701 original-DOL comparisons** on the candidate
and a fresh reference object. `GXClearGPMetric` also matches the linked
original instruction for instruction.

The SDK's register macros cast a computed pointer again before accessing it.
Pointer resolution now admits add/subtract expressions beneath an access cast,
using the existing typed arithmetic evaluator. Constant displacement folding
keeps arithmetic stride separate from access width: `(short *)((int *)p - 3)`
accesses twelve bytes before `p`, even though the eventual store is a halfword.
The previous helper incorrectly treated that displacement as three bytes.
Global pointers and frame-backed pointers load their current address value;
automatic arrays use their frame storage and element/row stride. The shared
load and store paths use the same helper. No GX-specific emitter is added.

Canaries **1927–1930** cover twelve computed-address functions and two frame
controls at O0/O4 across fifteen builds. The frozen `0a273dff` baseline compiles
**0/60** complete objects; candidate and fresh references compile **60/60**.
**11/60 whole objects** and **220/420 function text plus symbolic relocation
comparisons** match exactly. All **215,040 candidate calls** and corresponding
reference calls pass their return, complete input-memory, and ABI checks.
Samples cover positive/negative offsets, different arithmetic/access widths,
byte-pointer casts, variable indices used again after a load, local arrays, and
a helper that changes an address-taken pointer before the subsequent access.

The full GXPerf panel checks all 1,456 GX context bytes, ordered hardware-write
addresses/widths/values (FIFO and command-processor registers), the register
bank image, GPR14–31, FPR14–31, SP, and LR. Its **3,677 `GXSetGPMetric` cases**
cover all old/new transitions for each metric selector, all pairs of new legal
selectors, and 1,024 arbitrary-word selector quadruples exercising default
paths. **1,024 `GXClearGPMetric` cases** vary the surrounding context. These
panels total **219,741 passing candidate calls**.

Full GXPerf is **5,336 ELF bytes / 2,192 text bytes**, SHA-256
`39bb0563857e472af0177aa35854d4fec27be672de538dd5edb1a0a6a4724dc2`.
The fresh reference is **4,104 ELF bytes / 2,136 text bytes**, SHA-256
`060c8bddb099bdb1ae1ec08f500efb8fb910cf227607d8d1b5904325c672ed91`.
`GXSetGPMetric` remains **2,176 versus 2,120 bytes**. The pinned
`docs/reference-layouts/bfbb-gxperf.json` reports **1/2 candidate** and **2/2
reference** exact linked functions. Eight candidate jump-table address
relocations remain unresolved because their images/targets differ from the
original; this is an explicit instruction-parity gap.

The linked-DOL checker now verifies initialized jump-table images after applying
word relocations to instructions within pinned function ranges. It rejects
unverified targets, misalignment, overlapping relocations, and ambiguous images.
For named small-data objects with both SDA bases configured, the verified
original section selects r2 or r13. **13 checker tests pass**; the previous
GXPixel layout retains 0/12 candidate and 12/12 reference exact functions with
no unresolved relocations.

Regression controls retain **300/300** exact memory objects; **1,097** unchanged
indexed objects, including **972** known exact results and **577** identical
declines; and cumulative metadata's **1,494** compiled, **1,487** unchanged,
**1,179** known exact, and **704** identical declines. Its seven historical
`1469` changes predate this checkpoint. All **1,501** compiling recent objects
are unchanged, with **89** identical declines among **1,590** pairs. The prior
seven GX objects and full AXVPB are byte-identical.

The full compiler library test run has **1,582 passes and one failure**:
`inline_expansion::tests::composes_zero_argument_embedded_asm_at_a_nested_call_site`.
A focused replay with the committed baseline source reproduces the same failure;
it remains an existing frontier. The remaining 1,582 tests pass again after
restoring this change. Full-project builds, six configured GX units, variable
indexed instruction schedules, and remaining reference-bug reproduction are open.

Artifacts: `target/computed-pointer-canaries/{results,reference-results,
reference-comparison,execution-results,frame-execution-results}.json`,
`target/computed-pointer-perf/{compilation-results,full-execution-results,
candidate-verified-linked-text,reference-verified-linked-text,
pixel-linked-regression}.json`, its two DOL fixture files,
`target/computed-pointer-{index,metadata,recent,gx-library,full-ax}/`, and
`target/computed-pointer-{tests,tests-final,baseline-inline-test,linked-tests}.log`.

## GC/1.1p1 O0 shared parameter-spill bug, 2026-09-07

The fourteen-function parameter-home cohort now produces a **byte-identical
reference ELF** on GC/1.1p1 O0: **14/14 function bodies and symbolic relocations**
match. All **14,336 candidate/reference execution comparisons** agree, including
helper arguments, return values, raw saved GPR/FPR images, SP/LR/PC, the stack
image, and execution faults. The frozen `9f45cda8` baseline declines this cohort.

This deliberately reproduces an original compiler bug. Unoptimized scalar
parameter stores share `8(r1)`, in declaration order. An integer parameter can
overwrite a float before conversion; reversing parameter declarations reverses
which value survives. A double store can overwrite saved r31 or the caller's
saved LR. An ordinary integer-call control demonstrates that this is not
specific to floating conversion. **10,944 reference cases fail the independent
C/ABI model**, and the candidate reproduces their observed behavior. These
are reference matches, not semantic-correctness passes.

A build-profile switch resolves into an O0-only `BugReproduction` quirk. Its
isolated emitter uses the shared conversion normalizer, call machinery, and
expression visitor. It represents overlapping source images directly instead
of giving them independently allocated frame slots. The admitted family has
one scalar call, an optional unsigned result local, and a scalar arithmetic
return; unrelated bodies fall through using a cloned trial generator. The
measured three-term sum retains the reference load/add schedule. Synthetic
names avoid source parameters and locals, with a collision canary.

Canaries **1923/1924** cover direct/local results, parameter ordering, float and
double inputs, arithmetic, narrow stores, and an integer call. Canaries
**1925/1926** retain repeated-conversion and parameter-promotion frontiers.
Across all four files and fifteen builds, compilation advances **0/60 to 1/60**;
these are focused diagnostics, not a corpus parity percentage. Fresh references
compile **59/60**. The complete GC/1.2.5n O0 single-call cohort hits wibo's missing
`FormatMessageA` import; all **14 isolated functions compile** under the same
reference flags. That runner failure remains separate from candidate declines.
Other builds' direct conversion-return expressions and repeated conversions
remain open.

Regression controls retain **300/300** exact memory objects and **1,097** unchanged
indexed objects, including **972** known exact results and **577** identical
declines. Cumulative metadata retains **1,494** compiled, **1,487** unchanged,
**1,179** known exact, and **704** identical declines; seven historical `1469`
changes predate this milestone. Of **1,530** recent pairs, **1,500** compile,
**1,499** are byte-identical, and **30** retain their diagnostics. The sole changed
object is GC/1.1p1 O0 canary 1916: `initialized` and `assigned` gain exact function
text/relocation matches and pass **2,048** further execution comparisons; its
six other functions are unchanged. This brings the focused execution total to
**16,384 matching calls per side**. The seven compiling BfBB GX translation units
and full AXVPB remain byte-identical. Tests
pass: **47** version tests and **675** structured compiler tests.

This is a bounded first implementation of the spill bug. Repeated calls,
promoted parameters, pointer stores, branch/switch conversions, and other saved
register overlap cases remain reproduction gaps. Full-project builds and the
remaining seven configured GX units remain open.

Artifacts: `target/shared-spill-canaries/{results,reference-results,
reference-comparison,execution-results}.json`,
`target/shared-spill-reference-isolation/results.json`,
`target/shared-spill-recent/reference-comparison.json`,
`target/shared-spill-recent-execution/`, and
`target/shared-spill-{index,metadata,recent,gx-library,full-ax}/`.

## Full GXPixel, implicit conversions, and live array indices, 2026-09-07

The complete, unmodified BfBB `GXPixel.c` compiles with the project's GC/1.2.5n
flags. Its **12 functions pass 12,288 original-DOL comparisons**, 1,024 cases
per function, on both the candidate and a fresh reference object. Configured
GX compilation advances from **6/14 to 7/14** translation units. This covers
fog and fog-range adjustment, blend/alpha/depth/pixel-format state, dithering,
destination alpha, field masks, and field mode.

The shared structured conversion pass now exposes implicit unsigned-word
conversions at local initializers, assignments, assignment expressions, scalar
stores, and returns (including guards and switch arms). It retains the explicit
cast path and uses destination types rather than inferring a conversion from
floating descendants alone. An early structured trial lets straight-line
hidden calls receive nonleaf frame/liveness planning after earlier specialized
owners have had first refusal. Other integer widths, implicit call-argument
conversions, and static-data initialization are outside this extension.

Member access through a global struct pointer now belongs to the address-cache
owner; passing the pointer itself still requires scalar value reuse. The shared
leaf CFG emitter can handle multi-statement guarded tails after leading stores.
Native pixel-format execution then exposed a separate destructive-index bug:
a legacy array-address schedule scaled `pix_fmt` in place before subsequent
comparisons and lookups consumed its original value. Address schedules now copy
reserved or named virtual indices before overwriting them, including ordinary
indexed stores. Leaf declaration initializers reserve inputs that the body will
consume before statement-level liveness reservations begin.

Canaries **1915–1920** cover fifteen functions at O0/O4 across fifteen builds:
implicit conversion destinations, global-pointer guarded members, repeated
array loads, indexed stores, and byte-index normalization. The frozen
`855109e4` baseline compiles **0/90** complete objects; candidate and fresh
references compile **90/90**. All **230,400 candidate calls** pass their return,
ordered FIFO, memory, and ABI models. **0/90 whole objects** and **25/450 function
text plus symbolic relocation comparisons** match exactly. Separate canaries
**1921/1922** retain a repeated-load-plus-index return frontier: all **30**
candidate/baseline pairs still decline with the scratch-expression diagnostic,
while the references compile. These are recorded gaps, not passing samples.

The reference execution panel has **3,392 failing cases**, all GC/1.1p1 O0.
Disassembly confirms overlapping parameter and saved-register spills at `8(r1)`.
For example, `initialized`/`assigned` spill f1 there, overwrite it with the
integer parameter, and then reload that integer bit pattern as a float for
`__cvt_fp2unsigned`. Other cases overwrite saved GPR/FPR images. These original
bugs remain explicit reproduction gaps; ordinary semantic correctness is not
bug-for-bug parity.

Native GXPixel tests compare all 1,456 context bytes, ordered FIFO write
widths/values, input buffers, GPR14–31, FPR14–31, SP, and LR. Fog cases include
both projection paths and equal near/far or start/end degeneracies, with finite
binary32 inputs. Pixel formats span 0–7; booleans are canonical. The conversion
helper executes original DOL code. Texture flushes use the original helper with
only its `__GXData` address reference relocated into the harness. This is not
an exhaustive floating-point or invalid-enum claim.

Full GXPixel is **4,360 ELF bytes / 2,004 text bytes**, SHA-256
`1e5e7c5a02c091537d6cc37e53f6e635bed941ff9f44500a1f10afa1032541b7`.
The fresh reference is **3,568 ELF bytes / 1,608 text bytes**, SHA-256
`794e10e14b6976632c8a98c7fe1946ef2791e16f37ac2b7e27e0207b694786aa`.
`docs/reference-layouts/bfbb-gxpixel.json` pins **0/12 candidate** and **12/12
reference** exact linked functions, with no unresolved relocations. The linked
checker now handles low/high/adjusted-high address relocations and initialized
`.data` images whose bytes, symbol range, and unique identity are verified. This
resolves the pixel-format lookup table despite different local-symbol ordinals.

Regression controls retain **300/300** exact memory objects, **1,097** unchanged
indexed objects with **972** known exact results and **577** identical declines.
Cumulative metadata retains **1,494** compiled, **1,487** unchanged, **1,179**
known exact, and **704** identical declines; its seven historical `1469` changes
predate this checkpoint. All **1,410** recent objects compile; **1,380** are
unchanged. Only the thirty reduced `GXGetYScaleFactor` objects change, and their
**30,720** candidate/reference/baseline calls pass. The full GXFrameBuf repeats
all **14,336** native comparisons successfully; its scale-factor function shrinks
from **684 to 676 bytes**. The five other previously compiling GX objects remain
byte-identical. Full AXVPB still compiles; its sole changed function,
`AXSetVoiceSrcRatio`, remains 128 bytes and only commutes two independent
prologue instructions, verified directly against the frozen object.

These panels total **287,744 passing candidate calls**. Tests pass: **665**
structured tests, **5** guarded-global classifier tests, and **10** linked-DOL
checker tests. Full-project builds, the remaining GX units, instruction parity,
and reproduction of reference bugs remain open.

Artifacts: `target/implicit-conversion-canaries/{results,reference-results,
reference-comparison,execution-results-0..5}.json`,
`target/implicit-conversion-pixel/{compilation-results,full-execution-results,
verified-linked-text,reference-verified-linked-text}.json`, its twelve DOL
fixture files, `target/implicit-conversion-scale-regression/`,
`target/implicit-conversion-framebuf/`, and
`target/implicit-conversion-{index,metadata,recent,gx-library,full-ax}/`.

## Full GXFrameBuf and structured runtime conversions, 2026-09-07

The complete, unmodified BfBB `GXFrameBuf.c` now compiles with the project's
GC/1.2.5n flags. All **14 functions pass 14,336 original-DOL comparisons**,
1,024 cases per function, on both the candidate and a fresh reference object.
This includes `GXGetYScaleFactor`, both copy commands, clear/filter setup, and
bounding-box clearing. The configured GX survey advances from **5/14 to 6/14**
translation units; the five earlier GX objects and full AXVPB are byte-identical.

The implementation exposes explicit float/double-to-unsigned casts as runtime
calls before structured liveness and frame planning. Operand classification
uses declarations and expression types, including floating locals, arrays,
dereferences, double members, and nested casts. Comparisons and logical results
remain integers. The shared call path handles unsigned additions of conversion
results. This normalization currently covers explicit casts, not every implicit
conversion, and the existing specialized conversion planners remain available.

Three control-flow/frame corrections complete the real unit:

- Loop-carried copy/call results retain their register home across entry and
  backedges. Ordinary self-updates retain their existing entry aliases.
- A constant-address register first established in an `if` body is discarded
  from the continuation's cache unless it also existed on the incoming edge.
  This fixes copy commands taking their `clear == 0` path.
- GPR save placement accounts for anonymous numeric-conversion scratch and its
  final displacement. Planned FPR saves permit GPR frame growth before those
  saves are emitted; already-emitted FPR saves still require their own owner.
  This prevents later register allocation from overlapping conversion scratch
  with saved GPRs.

Canaries **1907–1914** contain 13 functions at O0/O4 across fifteen builds.
The frozen `98d5d1d4` baseline compiles **60/120** pairs; the candidate and fresh
references compile **120/120**. All **215,040 candidate calls** pass their value,
ordered call/FIFO, input-memory, and ABI models. The baseline's **92,160** calls
contain **38,160** failing cases: **26,880** conditional-port cases and **11,280**
loop-home cases. Known broken baseline loops use a bounded instruction budget.
**0/120 whole objects** and **15/390 function text plus symbolic relocation
comparisons** match the reference exactly.

The **215,040 reference calls** agree on return values and traces, but **1,536**
fail ABI preservation. All occur in GC/1.1p1 O0: 512 each in `local_sum`,
`member_sum`, and `until`. Disassembly confirms an incoming parameter spill
at `8(r1)` overwrites the saved r30 slot, which the epilogue subsequently loads.
These version-specific behaviors remain explicit reproduction gaps; passing
the ordinary ABI model does not constitute bug-for-bug parity.

Native comparisons use the original DOL `__cvt_fp2unsigned` and
`__GetImageTileCount`, not replacement arithmetic implementations. They check
return GPR/FPR bits where applicable, all 1,456 GX context bytes, ordered FIFO
write widths/values, input buffers, GPR14–31, FPR14–31, SP, and LR. Scale-factor
inputs use finite valid dimensions; vertical scales span 1 through 256. Clear,
AA, and vertical-filter arguments use canonical booleans. Render-mode data
objects are compiled but are outside the execution/linked-function claim.
Cross-version probes additionally check the modeled paired-single second lanes
with GQR0; this is not validation of unsupported quantized or paired arithmetic.

Full GXFrameBuf is **7,312 ELF bytes / 4,048 text bytes**, SHA-256
`1e4f7dec05630c32d8a1e585a093b82ae9965ecfa14dfe04e480c1ca29f4f1a1`.
The fresh reference is **5,248 ELF bytes / 2,972 text bytes**, SHA-256
`845d874e673adc76c1b02b7d7d48ee8015cc0424a5d33c5e3c2f608ca9138d2b`.
`docs/reference-layouts/bfbb-gxframebuf.json` pins **0/14 candidate** and
**14/14 reference** exact linked functions, with every relocation resolved.
`GXGetYScaleFactor` is **684 versus 568 bytes**. Instruction parity, the remaining
GX units, and full-project builds remain open.

Regression controls retain **300/300** exact memory objects, **1,097** unchanged
indexed objects with **972** known exact results and **577** identical declines.
Cumulative metadata retains **1,494** compiled, **1,487** unchanged, **1,179** known
exact, and **704** identical declines; the seven historical `1469` changes
predate this checkpoint. All **1,290** recent objects compile; **1,260** are
unchanged. The other 30 change only `outer` in canaries 1901/1902. Its **30,720**
candidate and reference calls pass; all eight earlier exact functions in those
objects remain exact. Including this rerun and native checks yields **260,096
passing candidate calls**. Tests pass: **664** structured tests, including the
three new conversion tests, and **3** allocation-frame tests. These focused
controls are not a whole-corpus parity estimate.

Artifacts: `target/runtime-call-canaries/{results,reference-results,
reference-comparison,execution-results-0..5}.json`,
`target/runtime-call-framebuf/{compilation-results,full-execution-results,
verified-linked-text,reference-verified-linked-text}.json`, its fourteen
original-DOL fixture files, and
`target/runtime-call-{index,metadata,recent,gx-library,full-ax}/`.

## GXFrameBuf vertical scaling and retained integer loops, 2026-09-07

BfBB's unmodified `GXSetDispCopyYScale` now compiles with the project's
GC/1.2.5n flags and passes **2,048 original-DOL comparisons**. A source slice
retains the first six copy-setup functions, the inline scanline helper, and
vertical scaling, omitting `GXGetYScaleFactor`. The six earlier functions also
pass their **6,144** original-DOL checks again. Vertical scaling checks the
returned height, all 1,456 context bytes, ordered FIFO write widths/values,
GPR14–31, FPR14–31, SP, and LR. Both compiled objects call the original DOL
`__cvt_fp2unsigned` through a trampoline; no replacement conversion model is
used. Inputs cover finite binary32 scales from 1 through 256 and randomized
context heights. Render-mode data objects and the complete translation unit
remain outside this execution claim.

Three shared paths account for this progress:

- Retained integer-valued inline definitions can compose local assignments,
  nested conditionals, and pretest loops through the existing statement lane.
  Locals are renamed per invocation; typed parameter captures preserve formal
  conversions and argument evaluation. Direct tail calls receive a local result
  destination after the caller's earlier guarded exits. Admission requires
  dominated reads, including the final return, and excludes memory effects,
  calls, mutable parameters, floating values, and nonlocal control transfers in
  the helper. It does not broaden automatic inlining of ordinary definitions.
- Branch comparisons between narrow and word register operands extend the
  narrow value using its declared signedness, then use the comparison selected
  by integer promotion rules. Both operand orders retain the word operand.
- Structured assignment aliases now preserve every live identity sharing a
  register before admitting a mutable copy. This includes transitive copies,
  later return/guard reads, and enclosing continuations of nested blocks.
  Nested blocks conservatively consult the full body until source liveness
  explicitly exposes those continuation and back edges.

Execution exposed the importance of the last rule: a first inline scanline
call could halve the caller's scale register before the second call consumed
it. Ordinary copy probes also expose incorrect branch joins and chained aliases.
The frozen `a871a122` baseline compiles **30/120** new sample/version pairs;
its **153,600** calls in the five ordinary copy probes contain
**20,820** model errors. The final candidate fixes these errors.

Canaries **1899–1906**, containing 33 functions at O0/O4 across fifteen builds,
compile **120/120** on both the candidate and fresh references. They cover the
real scanline helper, repeated calls, changing values, guarded tail returns,
name collisions, narrow formal parameters, signed actual arguments, sixteen
mixed-width comparison shapes, and five copy-survivor shapes. All
**1,013,760 candidate calls** and **1,013,760 reference calls** pass their
explicit integer/ABI models and agree. Including the **8,192** original-DOL
calls gives **1,021,952 candidate calls** in this checkpoint. **4/120 whole
objects** and **72/990 function text plus symbolic relocation comparisons**
match exactly; semantic agreement is not instruction parity.

The configured seven-function slice is **3,624 ELF bytes / 1,308 text bytes**,
SHA-256 `bfd4e5866446c12953aad59aae5e04747f7454bcf7bedb0043f40aeb3206b665`.
The fresh reference is **2,616 ELF bytes / 932 text bytes**, SHA-256
`85bc1a1c9887946eab6a173ed5597ba47c3b1a4d16d5407985507caca7365ace`.
`docs/reference-layouts/bfbb-gxframebuf-copy-scale.json` pins **0/7 candidate**
and **7/7 reference** exact linked functions, with every relocation resolved.
Vertical scaling is **244 versus 204 bytes**. The linked checker now accepts
external function names, verifies their ranges against the pinned symbol map,
and resolves their calls without counting those helpers as candidate functions.

Full GX remains **5/14**. `GXGetYScaleFactor` advances through inline expansion
and mixed-width comparisons, then declines with “allocated callee-saved values
need a canonical frame owner.” Its hidden float-to-unsigned runtime calls still
need consistent call-liveness and frame planning. All five compiling GX objects
and full AXVPB remain byte-identical. No full-project or whole-corpus claim is
made.

Regression controls retain **300/300** exact memory-operand objects, all
**1,170** recent objects (1821–1898), **1,097** unchanged indexed objects with
**972** known exact results and **577** identical declines. Cumulative metadata
retains **1,494** compiled, **1,487** unchanged, **1,179** known exact, and
**704** identical declines; its seven historical `1469` changes predate this
checkpoint. Tests pass: **121** inline tests, including three new admission and
hygiene tests, **4** value-version tests, and **7** linked-DOL tool tests. The
previously documented embedded-asm composition test remains excluded; a broad
inline run reproduced that existing failure before the focused passing run.

Artifacts: `target/inline-scalar-loop-canaries/{results,reference-results,
reference-comparison,execution-results}.json`,
`target/inline-scalar-loop-framebuf/{compilation-results,execution-results,
prefix-execution-results,verified-linked-text,reference-verified-linked-text}.json`,
its seven original-DOL fixture files, and
`target/inline-scalar-loop-{index,metadata,recent,gx-library,full-ax}/`.

## GXFrameBuf copy setup and cyclic argument preservation, 2026-09-07

The unmodified first six functions of BfBB `GXFrameBuf.c`, compiled as a source
prefix with the project's GC/1.2.5n flags, now pass **6,144 original-DOL
comparisons**: 1,024 each for display/texture copy source and destination,
frame-to-field mode, and copy clamp. The complete unit advances to the skipped
nested-loop inline helper `__GXGetNumXfbLines` in `GXGetYScaleFactor`; it still
does not compile, and the complete GX survey remains **5/14**.

This milestone extends three shared lowering paths:

- Discarded integer unary expressions retain their arithmetic at O0. Optimized
  code removes the unary operation while keeping calls and memory operands;
  resident scalar values and nonvolatile scalar globals need no evaluation.
  Floating operands and more elaborate unused-expression optimization remain
  outside this focused extension.
- Address-taken scalar variables participate in arithmetic as frame loads,
  including narrow promotion. A helper can overwrite their stack slots through
  escaped pointers; register homes cannot substitute for those stored values.
  Passive frame mirrors and existing register-only arithmetic retain their owners.
- The general argument scheduler handles leaf-register permutations and narrow
  formals, with independent frame-address arguments. It emits ready moves first
  and breaks cycles by preserving a source in a virtual temporary. Narrowing an
  argument in its own register also counts as a write: a later full-width alias
  must be consumed before those upper bits disappear. Existing two-word swaps
  and acyclic word-expression schedules keep their selection precedence.

The first executable prefix passed final GX-state comparisons but **1,008 of
1,024 texture-destination calls passed the wrong height** to the tile helper.
The unused column count hid the error. The final harness compares the helper's
actual format/width/height arguments as well as all 1,456 context bytes and
preserved GPRs/SP/LR. Both sides execute the original tile-count helper from the
DOL, with the candidate reaching it through a branch trampoline. There are no
replacement tile calculations in this prefix validation.

The prefix candidate is **3,120 ELF bytes / 1,064 text bytes**, SHA-256
`5f5160be177f851e350f176a2341ad4f6cdf6d5ac84c30dfc36edddb6b153b40`;
the fresh configured reference is **2,184 ELF bytes / 728 text bytes**, SHA-256
`aadf3ff38309bb1f3d478c7bbdc6160bef15e1db77c8a759c13ac14669563637`.
The pinned `docs/reference-layouts/bfbb-gxframebuf-copy-setup.json` records
**0/6 exact linked functions**. Texture destination is 372 candidate bytes
versus 304 original bytes; the linked-text checker also leaves its external tile
helper relocation unresolved. Prefix execution and instruction parity are
separate measurements; the four render-mode data objects are not validated here.

Canaries **1891–1898** compile **120/120** sample/version pairs, up from
**0/120** with frozen baseline `7d4466b6`; all fresh references compile
**120/120**. The 27 functions cover unary effects, eight escaped-scalar
arithmetic shapes, ten argument permutations/conversions, and the real texture
copy-destination reduction. **12/120 whole objects** and **238/810 function text plus symbolic relocation comparisons** match exactly. All **414,720 candidate calls**
pass their integer/access/call models, plus the **6,144** original-DOL prefix
calls: **420,864 candidate calls** in this checkpoint. Explicit helper bodies
supply reduced fill/call-result samples; the texture reduction uses the original
DOL helper and context fixtures.

The reference panel executes the same **414,720 calls**. **4,604** expose a
GC/1.1p1 O0 fidelity gap: **4,096** permutation calls overwrite multiple
parameters in the same `8(r1)` spill slot, and **508** texture-destination calls
pass wrong tile arguments (**494** also change the final context). Fresh
disassembly confirms the repeated stores and reloads at the same stack offset.
The candidate follows the explicit models and original-DOL fixtures here but
does not yet reproduce that version's spill-slot defect. The other **410,116**
reference calls pass and agree with the candidate; these exceptional rows are
retained as non-matches, not counted as reference parity.

Fresh O1/O2/O3 probes of the eight unary functions compile **45/45** per side,
with **38/45 whole objects exact**. They confirm that optimized discard begins
at O1. Cached regressions retain **300/300** exact memory-operand objects,
**1,097/1,097** compiled indexed objects unchanged, **972** known exact results,
and **577** identical declines. Cumulative metadata retains **1,494** compiled,
**1,487** unchanged, **1,179** known exact, and **704** identical declines; its
seven changed historical `1469` shift objects predate this milestone. All
**1,050** recent sample/version objects (1821–1890), all five compiling GX
units, and full AXVPB remain byte-identical to the previous compiler. Focused
checks pass: **12** argument-scheduler, **32** frame, **40** object, and **46**
version tests. No whole corpus or full project holdout was rerun.

Artifacts: `target/discarded-unary-canaries/{results,reference-results,
reference-comparison,execution-results}.json`,
`target/discarded-unary-framebuf/{compilation-results,execution-results,
verified-linked-text}.json`, its six original-DOL fixture files,
`target/discarded-unary-probes/level-results.json`, and
`target/discarded-unary-{index,metadata,recent,gx-library,full-ax}/`.

## Computed integer equality from GXFrameBuf, 2026-09-07

The configured BfBB `GXFrameBuf.c` advances past
`__GXData->cpTexZ = (fmt & 16) == 16` in `GXSetTexCopyDst`. Its next decline is
the discarded `!peTexFmt;` expression. The full unit still does not compile;
the fourteen-unit GX survey remains **5/14**. This checkpoint adds a shared
comparison path, rather than a function-specific GX replacement.

Computed integer operands now materialize once before comparison with a small
nonzero constant, including arithmetic, masks, casts, calls, and conditional
values. Existing register-leaf, direct-load, zero-equality, and floating paths
retain their selection precedence. Narrow call returns are promoted from their
declared low bits before comparison; poisoned upper bits exposed **1,260 wrong
results** in the first candidate, all resolved in the final panel. Conditional
volatile operands retain their guarded access and calls execute once.

`ComputedConstantEqualityStyle` separates three measured O3/O4 choices:
early GC through 1.3.2r retains the masked-value/add-negative-constant sequence;
GC 2.0–2.7 uses `subfic`; 4.x uses `addi` when the negated immediate fits.
Later profiles extract a matching single-bit mask directly. The divergent
choices have named, inspectable intentional quirks. O0–O2 retain a constant
register and `subf`; O0 keeps computed temporaries out of operand homes, while
O1/O2 permit coalescing. Representable narrow constants retain their redundant
O0–O2 conversion. The `-32768` boundary avoids overflowing the negated immediate.

Canaries **1883–1890** compile **120/120** sample/version pairs, up from
**0/120** with frozen baseline `b8c8ef8e`; fresh references compile **120/120**.
The panel contains 28 functions at O4 and O0 across all fifteen builds.
**30/120 whole objects** and **506/840 function text plus symbolic relocation
comparisons** match exactly. Both core eight-function objects match for every
build: **30/30**. The additional call, store, conditional, narrow, reused-value,
and immediate-boundary cases remain separately measured; execution agreement
does not imply their instruction parity.

All **430,080 candidate calls** and **430,080 reference calls** pass the
explicit integer/access model and agree with each other. Each function runs
512 inputs, including masks and comparison boundaries, upper-bit-poisoned
narrow call returns, canonical narrow parameters, and random words. The harness
checks return values or complete state bytes, ordered reads/writes, call counts,
preserved GPRs, and SP/LR restoration. Small explicit external helper bodies
supply call results; this is a reduced-sample execution panel, not a linked
original-DOL validation of `GXSetTexCopyDst`.

An additional fresh O1/O2/O3 panel compiles **45/45** on each side, with
**19/45 whole objects exact**. Lower optimization levels still differ in
legacy operand ordering and the O2 single-bit extraction threshold; Wii O3
also differs in object layout. These are recorded non-matches, not evidence
that every optimization level has the O4 schedule. All **184,320 additional calls per side** pass the same integer and ABI
checks, bringing this checkpoint to **614,400 candidate calls** validated
against references and explicit models.

Cached regressions retain **300/300** exact memory-operand objects and
**1,097/1,097** compiled indexed objects unchanged, including all **972** known
exact results; **577** declines remain identical. The cumulative metadata panel
retains **1,494** compiled objects, **1,487** unchanged, **1,179** known exact,
and **704** identical declines. Its seven changed historical `1469` shift
objects predate this checkpoint. All five compiling GX units and full AXVPB
retain their previous object hashes. Focused tests pass: **46** version/profile,
**11** comparison, **32** frame, and **40** object tests. No full corpus or full
reference-project holdout was rerun.

Artifacts: `target/computed-equality-canaries/{results,reference-results,
reference-comparison,execution-results}.json`,
`target/computed-equality-levels/`, `target/computed-equality-gx-library/`,
`target/computed-equality-{index,metadata,full-ax}/`, and
`target/memory-operands-probes/computed-equality-results.json`.
The full GXFrameBuf discard/loop/switch frontier and the nonmatching reduced
object schedules remain follow-up work; no new original-DOL exactness is claimed.

## Complete configured GXLight and versioned floating negation, 2026-09-07

The complete BfBB **`GXLight.c` now compiles and links its required local
helpers with the project's GC/1.2.5n configuration**, advancing the fourteen-unit
GX compilation survey from **4/14 to 5/14**. All **twelve light functions pass
1,024 comparisons each against the original DOL**: **12,288 candidate calls**
cover attenuation, spotlight and distance calculations, position/direction/color,
paired-single FIFO uploads, and channel colors/count/control. Both sides execute
the original cosine implementation; the candidate reaches it through a branch
trampoline, with no replacement arithmetic or function stubs in this full-unit
comparison. Floating arguments and light data are finite binary32 values.

The candidate is **4,400 ELF bytes / 1,796 text bytes**, SHA-256
`b2f9427e383c1bb43ab6def8e60c9bd31b75f0bedcc5a381b6d2cc18feb1b6b7`.
A fresh configured reference compilation produces **3,928 ELF bytes / 1,556 text
bytes**, SHA-256
`4bdf9aa4ee178e8a69196fc8f624ed6317fedc0bd98bfdad149af008355efb01`.
The pinned `docs/reference-layouts/bfbb-gxlight.json` reports **0/12 exact linked
functions**; unresolved literals and relocations remain non-matches. Spotlight
text is 432 candidate bytes versus 400 original bytes. Execution agreement on
these inputs does not imply instruction parity or whole-project completion.

Frozen baseline `2f786142` declines at the spotlight's `-cr * a1`. Shared fixes
now provide:

- Negated register arithmetic with measured operand order. The 2.x optimizer
  cancels paired product negations and normalizes negative sums through the
  existing subtraction/contraction selector. O0 and 4.x retain explicit
  operations. A named profile decision and an inspectable behavior quirk select
  this difference; it affects NaN signs and fused rounding, not just scheduling.
  O1/O2/O3 reference probes confirm the optimized generation split. Existing
  memory and call specializations retain their earlier selection paths.
- The pure `__cntlzw` intrinsic, classified once for frame planning, symbol
  traversal, and instruction selection. Nested, loaded, computed, constant-zero,
  and call-result operands use the shared integer operand evaluator. The light
  upload no longer emits an unresolved external intrinsic call.
- Integer register locals in the existing parameterized assembly binder. Their
  virtual GPR lifetimes coexist with pointer arguments and symbolic FPR locals,
  allowing the real mixed-register `PushLight` body to expand at the call site.
- Assignment-time local allocation that retains the declared register class.
  A float local first materialized there previously received a GPR identity;
  its later call argument then converted unrelated integer bits. The spotlight
  cosine argument and the reduced forwarding samples now keep their FPR value.

Canaries **1871–1882** compile **180/180** sample/version pairs, compared with
**90/180** baseline successes; all fresh references compile **180/180**. Baseline
compilation of the intrinsic and mixed-assembly samples still leaves unresolved
helper symbols, so those successes are not executable parity. The four core
float samples are **60/60 whole-object exact** across all fifteen versions,
O4/O0, and contraction on/off. Across all twelve samples, **60/180 whole
objects** and **1,064/1,410 function texts plus symbolic relocations** match.

Candidate execution validation covers **1,382,400 arithmetic/intrinsic/spotlight/
local-argument calls**, **15,360 mixed-assembly calls**, and the **12,288** full-unit
calls: **1,410,048 calls** in total. Arithmetic checks compare exact FPR result
bits and FPSCR against reference instructions, including signed zeros,
subnormals, large finite values, infinities, and NaN payloads. Local forwarding
also has an explicit rounding model. Spotlight reductions reuse original-DOL
fixtures. Mixed-assembly checks include the intermediate store sequence, final
memory, and register preservation. The sample's external `fetch` and identity
acceptors are controlled ABI probes; they are separate from full-unit validation.

Cross-version parity still has measured gaps, retained in the result artifacts:

- **5,120 GC/1.1p1 O0 calls** expose reference spill corruption in the spotlight
  and local-argument samples. Saved GPR/FPR homes are overwritten by parameter
  spills; the switched forwarding function also has **1,024 result mismatches**.
  Candidate values pass the original spotlight fixtures or explicit rounding
  model, but do not reproduce this compiler bug.
- **256 GC/1.3 optimized FIFO-copy calls** reproduce the previously observed
  incorrect paired-single displacement folding in the reference. This version's
  behavior remains a candidate mismatch.
- **1,536 optimized 4.x assembly calls** differ in their intermediate store
  sequence: references eliminate overwritten stores to the helper's non-volatile
  destination. The candidate preserves the source assembly sequence. Matching
  this optimization remains work; final memory agrees on these inputs.

The paired-single probe helper now also executes non-updating indexed `psq_lx`,
including an r0 index and the RA=0 absolute-base rule. This removes the indexed
callee-save-restore harness limitation for the new samples. Explicit zero GQRs
and finite lanes remain required; quantization, updating transfers, indexed
stores, paired arithmetic, and non-finite paired operands remain unsupported.
Full-unit checks execute **12,288 modeled paired instructions on each side** and
validate FIFO writes, complete GX context and light memory, callee-saved GPRs and
first FPR lanes, stack restoration, and return control flow.

Focused checks pass: **45** version, **4** intrinsic, **3** floating-negation,
**2** parameterized-assembly, **31** frame-convention, **40** object, and **8**
paired-single probe tests. Cached panels retain **300/300** memory-operand object
matches; **1,097** unchanged indexed-panel objects, **972** known reference
matches, and **577** identical declines; and **1,494** cumulative compiled
objects, **1,487** unchanged, **1,179** known matches, and **704** identical
declines. The seven historical variable-shift changes predate this checkpoint.
Full GXBump, TEV, GXTransform, and AXVPB object hashes remain unchanged. The full
corpus was not rerun.

Artifacts are `target/negated-float-canaries/{results,reference-results,
reference-comparison,execution-results,asm-execution-results}.json`,
`target/negated-float-light/{execution-results,verified-linked-text}.json`, and
`target/negated-float-{gx-library,index,metadata}/results.json`. Drivers are
`target/check_negated_float*.py`, `target/recheck_negated_float*.py`,
`target/measure_negated_float.py`, and `target/probe_negated_float*.py`.

## Complete configured GXTransform compilation and execution, 2026-09-07

The complete BfBB **`GXTransform.c` now compiles with the project's GC/1.2.5n
configuration**, advancing the fourteen-unit GX compilation survey from
**3/14 to 4/14** (`GXBump`, `GXDisplayList`, `GXTev`, and `GXTransform`). All
**fourteen functions pass 1,024 execution comparisons each against the original
DOL**: **14,336 candidate calls**, including matrix uploads, projection getters
and setters, scissor controls, clip mode, viewport operations, and matrix-index
updates. Internal calls execute their actual instructions; there are no function
stubs in this configured-unit comparison.

The candidate is 3,832 ELF bytes / 1,708 text bytes, SHA-256
`0d771ac3d55b325ded722807ad77221a63dded0e20cc6df246a64b13fddb757b`.
A fresh configured reference compilation produces 3,064 ELF bytes / 1,428 text
bytes, SHA-256
`61de4d87bb96089a90a68c039aba8eba3dbf83fb1ba2c4ec3ae6381af6dcbeec`.
**0/14 functions match linked instruction bytes** using the new pinned
`docs/reference-layouts/bfbb-gxtransform.json`. Unknown literal relocations
prevent exact claims where unresolved. Matrix upload sizes are 92/80, 96/80,
and 192/180 candidate/reference bytes; matching their schedules remains work.

Frozen baseline `0d89b328` declines on an absolute member address in the first
matrix upload. Three shared lowering fixes enable the full unit:

- Constant aggregate-pointer member addresses fold their field/index displacement
  with 32-bit wrapping arithmetic, including a low-half carry and destinations
  using r0. Variable indexes materialize the absolute base in a virtual register.
- Address-of indexed/member expressions expose their input registers to dependency
  collection. Pure indexed-address call arguments use the existing dependency
  scheduler, preserving a pointer before another argument occupies its ABI home.
  This fixes the projection getter's overwritten destination pointer.
- Structured if/else arms begin with the dominating constant-address cache and
  intersect their outgoing caches at the join. A hardware base created only in
  the then arm can no longer leak into the else arm or following code. This fixes
  the matrix-index update's missing FIFO base initialization.

The new `tools/ppc_paired_single.py` instruction hook models only non-updating,
immediate `psq_l`/`psq_st` with explicitly zero GQR values and finite binary32
lanes. It handles W=0 pairs and W=1 single transfers, including signed zero and
subnormals. Native Unicorn executes ordinary scalar FP instructions; PS1 is
tracked separately. Quantization, paired arithmetic, indexed/updating transfers,
and non-finite operands are outside this model. Configured-unit comparisons
execute **53,248 modeled instructions on each side** and check FIFO write order,
width and value, the whole 1,456-byte context, matrix/output memory, callee-saved
GPRs and first FPR lanes, stack restoration, and return control flow. They do not
claim arbitrary floating-point or paired-lane ABI coverage.

Canaries **1861–1870** cover absolute/nested/indexed member addresses, call
argument dependencies, overlapping inline-assembly copies, the three real
matrix-upload reductions, and shared/different/nested hardware-address branches
at O4/O0. Candidate compilation advances **60/150 → 150/150** across fifteen
versions. Fresh references compile **146/150**: GC/3.0a3 and GC/3.0a3p1 decline
both matrix-upload samples over a const array-pointer argument type. Candidate
acceptance remains a diagnostic mismatch for those four pairs. Among reference
successes, **0/146 whole objects** and **117/558 function texts plus symbolic
relocations** match.

All **215,040 candidate sample executions pass**, for **229,376 new candidate
calls** including the configured full unit. Matrix samples reuse original-DOL
fixtures; other samples use explicit address, copy, call-argument, and FIFO
models. Only the sample's external `take` function is replaced by argument
capture. The baseline records **46,080 failures in 53,760 calls**. Reference
execution records **8,704 non-passing calls out of 202,752**, so this is not an
all-version execution-parity claim:

- GC/1.1p1 O0 `variable_index` has **256 mismatches**: both incoming parameters
  spill to the same slot, producing `5*i` instead of `p + 4*i`. Reproducing this
  original compiler bug remains required work.
- GC/1.3 optimized matrix uploads have **3,072 mismatches**. Its inline assembly
  folds the FIFO's 16-bit displacement into a paired-single instruction's
  narrower displacement/control fields, changing the destination/transfer.
  This original behavior is not yet reproduced.
- GC/3.0a3, GC/3.0a3p1, and Wii/1.0 O0 copies, plus Wii/1.0 O0 matrix uploads,
  account for **5,376 incomplete comparisons**. Their indexed paired-single
  callee-save restores raise unsupported CPU exceptions in this harness;
  they are recorded as unvalidated, not compiler mismatches or passes.

Cached regression panels retain **300/300** memory-operand object matches,
**1,097** unchanged compiled indexed-panel objects (including **972** known
reference matches), and **577** identical declines. The cumulative metadata
panel retains **1,494** compiled objects, **1,487** unchanged from its older
baseline, **1,179** known matches, and **704** identical declines; its seven
historical variable-shift changes predate this checkpoint. Full GXBump, TEV,
and AXVPB object hashes remain unchanged. Focused tests pass: **3** dependency,
**20** member-address, **4** structured-if/else, **31** frame-convention,
**40** object, and **6** paired-single model tests. The full corpus was not rerun.

Artifacts are `target/absolute-member-canaries/{results,reference-results,
reference-comparison,execution-results}.json`,
`target/absolute-member-transform/{execution-results,verified-linked-text}.json`,
and `target/absolute-member-{gx-library,index,metadata}/results.json`.
Drivers are `target/check_absolute_member*.py`,
`target/measure_absolute_member.py`, and `target/probe_absolute_member*.py`.

## Affine byte field updates and first TEV byte match, 2026-09-07

The configured BfBB **`GXSetNumTevStages` now matches all 40 original linked
instruction bytes**, improving complete `GXTev.c` from **0/16 to 1/16** byte
matches. Its verified linked SHA-256 is
`f6a26aef4fdc9ace727f3f7bfe54bcf2d6160e2ec7985fde7df9a67057926022`.
The new `docs/reference-layouts/bfbb-gxtev.json` pins the original DOL, symbol
map, and `__GXData` placement for the existing linked-text checker. Unknown
relocations in other TEV functions prevent exact results; the layout does not
claim those functions match.

Frozen baseline `2a6a5a00` emits 56 bytes for this function. The shared
`global_bitfield_dirty_update` recognizer now accepts a byte parameter plus or
minus an immediate in an intrinsic field insertion. The emitter promotes the
byte before applying the adjustment and uses the existing field-store and
dirty-mask schedule. The signed 16-bit immediate bound, explicit narrow casts,
volatile pointer exclusion, and distinction from the legacy C shifted-OR form
remain explicit. No function name or GX field offset selects this extension.

Only `GXSetNumTevStages` changes in the complete TEV object's function text and
symbolic relocations; the other fifteen functions remain unchanged. The whole
candidate is now 4,624 ELF bytes / 2,448 text bytes, SHA-256
`ff6a1dd84ad3d60c8e328ae95f59ca2039bf5fbe79332931fc218d50c43659fc`.
All sixteen functions again pass **16,384 original-DOL execution comparisons**.
The GX compilation survey remains **3/14**. Full `GXBump.c` and `AXVPB.c`
retain their previous whole-object hashes, including all eight exact linked
GXBump functions.

Canaries **1853–1860** cover the configured count update, constant and mutable
global pointer bindings, positive/negative adjustments, signed immediate
limits, wrapping insertion masks, overlapping field/dirty storage, explicit
byte narrowing, an out-of-range immediate, volatile pointer accesses, and
absolute global addressing at O4/O0. Baseline, candidate, and fresh reference
compilers all compile **120/120** sample/version pairs. Whole-object matches
advance **0/120 → 23/120**, and function text plus symbolic relocation matches
advance **0/450 → 71/450**. Remaining mutable-pointer reload and version-specific
promotion/scheduling differences are not claimed exact.

Each of baseline, candidate, and reference passes **138,240 sample executions**
over valid byte arguments, randomized prior field values, complete context
comparison, volatile pointer read counts, callee-saved registers, and caller
argument storage. The TEV-count samples use **30,720 original-DOL fixtures**;
the remaining **107,520** calls use explicit rotate/insert and dirty-word models.
Together with the configured full-unit comparison, new candidate validation is
**154,624 calls**. Later MWCC versions omit the incoming byte mask in some
arithmetic forms, so arbitrary high register bits are not part of this
cross-version byte-argument execution claim.

Cached regression panels retain **300/300** memory-operand object matches,
**1,097** unchanged compiled indexed-panel objects (including **972** known
reference matches), and **577** identical declines. The cumulative metadata
panel is unchanged from the preceding checkpoint: **1,494** compile, **1,487**
remain byte-identical to its older baseline, **1,179** known matches remain,
and **704** declines remain identical. Its seven historical variable-shift
changes are from the preceding milestone. Focused tests pass: **4** intrinsic,
**40** object, and **6** linked-DOL checker tests. The full corpus was not rerun.

Artifacts are `target/tev-count-canaries/{results,reference-results,
execution-results,reference-comparison,baseline-reference-comparison}.json`,
`target/tev-count-tev/{full-execution-results,verified-linked-text}.json`, and
`target/tev-count-{gx-library,index,metadata}/results.json`. Drivers are
`target/check_tev_count*.py`, `target/measure_tev_count*.py`, and
`target/probe_tev_count*.py`.

## Complete configured TEV compilation and execution, 2026-09-07

The complete BfBB **`GXTev.c` now compiles with the project's GC/1.2.5n
configuration**, advancing the fourteen-unit GX compilation survey from
**2/14 to 3/14** (`GXBump`, `GXDisplayList`, and `GXTev`). Frozen baseline
`6e57cf1e` declines in `GXSetTevOrder`. All **sixteen TEV functions pass
1,024 execution comparisons each against the original DOL**, with no function
stubs: **16,384 candidate calls** validate FIFO write order, width and value,
the full 1,456-byte context, unchanged input aggregates, callee-saved GPRs/FPRs,
stack restoration, and return control flow. This is execution agreement on
these inputs, not byte parity or a whole-project claim.

The complete candidate is 4,680 ELF bytes / 2,464 text bytes, SHA-256
`1e2c3bbf80854d315c2c9eb69d9827037ea2d760ab507ab6c05ebe65b26ef841`.
A fresh configured reference compilation produces 3,840 ELF bytes / 1,892 text
bytes, SHA-256
`8ca1794362db022a3dfdd04ff79f9bdf634fd87fc2e98192230bc7108da0bd09`.
**0/16 functions match instruction bytes and symbolic relocations.** In
particular, `GXSetTevOrder` is 452 vs 412 bytes and `GXSetTevColor` is 156 vs
124 bytes. Matching their schedules remains work to do.

The shared lowering changes are:

- Integer selects whose arms cannot use the existing register-phi schedule
  retain a branch diamond. Only the selected arm is evaluated, and branch
  boundaries retire conditional caches through the common control-flow helper.
  Existing speculative arithmetic selects now reject memory-reading or
  side-effecting false arms. A branchless mask also declines when its value
  and mask would both occupy r0 in a store.
- A variable left shift with a constant source materializes the source in its
  own virtual register, preserving leaf or computed shift counts. Other shift
  operators and existing earlier measured schedules retain their owners.
- The parser remembers aggregate value parameters before ABI lowering. Their
  explicit address is the incoming aggregate address, with a pointer cast
  preserving `sizeof` and arithmetic stride. Declared pointer parameters still
  take the address of their pointer storage. Pointer access accepts nested
  pointer casts through the existing computed-address resolver. This fixes
  `GXSetTevColor`, whose otherwise-unused `rgba = *(u32*)&color` previously
  caused later channel loads to read bytes of the ABI pointer.

Canaries **1841–1852** cover nested/computed selects, destination aliases,
volatile and invalid unselected memory arms, narrowed values, constant variable
shifts, the full TEV order reduction, aggregate value/pointer distinction,
function-scope identity reset, and aggregate address size/stride, at O4 and O0.
Across fifteen compiler versions, compilation advances **60/180 → 180/180**.
Candidate execution passes **199,680 calls**. The baseline's compiled samples
execute 69,120 calls, including **15,360 aggregate-address failures**.

The previously blocked reference runner recovered during this checkpoint.
Fresh MWCC compilation succeeds for **180/180** new sample/version pairs, and
those reference objects independently pass the same **199,680 execution
checks**. Reference O0 code uses original DOL EABI GPR save/restore routines,
relocated as an instruction bank; these are not stubbed. Checks permit the
callee's linkage-area writes while preserving caller argument storage.
Whole-object equality is **2/180**, and function text plus symbolic relocation
equality is **173/690**. These are new-sample diagnostics, not corpus estimates.

Focused regression checks retain **300/300** cached memory-operand object
matches and **1,097/1,097** compiled indexed-panel objects, including **972**
known reference matches; all **577** declines are unchanged. The cumulative
metadata panel compiles **1,494/2,198**, retains **1,179** known reference
matches, and keeps **704** declines unchanged. Seven previously nonmatching
`1469_global_computed_operands` objects change under the constant-shift path;
all other **1,487** compiled objects remain unchanged. Those seven versions
pass **8,960 calls each** for baseline, candidate, and fresh reference objects.
Total new candidate execution validation is **225,024 calls**.

Full `GXBump.c` remains byte-identical to the checkpoint with all eight linked
functions exact (`411f42222cf4814ac1272149fd7319eb67afdfb5413e0d29a42b38e9cfc505e0`).
Full `AXVPB.c` also remains byte-identical
(`1a8fed48a1634517cd66e23f09754e75274d170ca826e61408ff96650605e7e3`).
Focused tests pass: 1 aggregate-parameter parser test, 44 control-flow tests,
6 pointer-expression tests, 3 structured-leaf tests, 17 switch-lowering tests,
31 frame-convention tests, and 40 object tests. The full corpus was not rerun.

Measurement artifacts: `target/tev-select-canaries/{results,execution-results,
reference-results,reference-execution-results,reference-comparison}.json`,
`target/tev-select-tev/{full-execution-results,reference-comparison}.json`,
`target/tev-select-gx-library/results.json`, `target/tev-select-index/results.json`,
and `target/tev-select-metadata/{results,changed-execution-results}.json`.
Drivers are `target/check_tev_select*.py`, `target/measure_tev_select.py`, and
`target/probe_tev_select*.py`. Original DOL and symbol-map hashes remain those
pinned in `docs/reference-layouts/bfbb-gxbump.json`.

## Global member-array bases and retained leaf values, 2026-09-07

The configured BfBB **first five `GXTev.c` functions now compile and execute
correctly against their original DOL code**. Frozen baseline `bc4936a9` declines
in `GXSetTevOp` because its embedded arrays use an uncached global struct pointer.
The configured five-function prefix now produces a 2,144-byte object with
784 text bytes, SHA-256
`c7bc17582465c56f1c7c2121b042c19a615f0dae45448bce9d4b888c534e81cc`.
These functions are **not instruction-byte matches**:

| Function | Candidate bytes | Original bytes |
| --- | ---: | ---: |
| `GXSetTevOp` | 216 | 140 |
| `GXSetTevColorIn` | 112 | 68 |
| `GXSetTevAlphaIn` | 112 | 68 |
| `GXSetTevColorOp` | 172 | 104 |
| `GXSetTevAlphaOp` | 172 | 104 |

A native survey compiles the fourteen GX translation units listed by the
project's `configure.py` using its GC/1.2.5n flags. Both checkpoints compile
**2/14 complete units**, `GXBump.c` and `GXDisplayList.c`; this milestone does
not claim a new complete GX unit. The first failure advances from `GXSetTevOp`
to the later `GXSetTevOrder`, where a conditional local assignment still needs
support. `GXTransform.c` advances from `GXSetProjection` to
`GXLoadPosMtxImm`, whose absolute member address needs support. `GXLight.c`
advances from `GXInitLightAttn` to `GXInitLightSpot`, whose negated floating
multiply needs operand-order support. These are compiler diagnostics, not
reference-object parity measurements.

The changes use shared lowering paths:

- `member_base_register` loads an uncached file-scope pointer into a fresh
  virtual instead of requiring a local register. Existing condition-cache
  values and shadowing rules remain in force. O0 member-array constant stores
  now use the same resolver. Scalar-pointer casts and SDA/absolute globals
  share the ordinary global-load policy.
- When the existing void-local substitution paths decline, supported leaf
  bodies can retain their locals through the structured statement emitter.
  This preserves memory snapshots across intervening writes or pointer
  replacement. The adapter reuses the existing local type and frame checks;
  it does not add a separate statement engine or call/frame policy.
- Indexed float/double copies keep the loaded value in an FPR and retain a
  computed destination index independently. Narrowed integer store values
  produced in r0 are saved before destination-index scaling overwrites r0.
- Pure intrinsics keep their virtual assignment destination. Call-shaped
  intrinsic syntax no longer selects the ABI-result optimization that forces
  r3 and can overwrite a live first parameter. Actual calls retain that path.

Twelve new canaries **1829–1840** improve **0/180 → 180/180 compiled objects**
across fifteen builds at O0/O4. They cover global embedded arrays, pointer
casts and shadowing, zero and variable stores, SDA/absolute addressing, signed
narrow snapshots, multiple retained values, global-pointer replacement,
float/double copies with computed indices, and the five complete TEV reductions.

Validation totals **343,040 passing candidate calls**:

- **337,920** corpus calls, including **153,600** TEV calls compared with
  original DOL fixtures across all builds. The remaining cases check independent
  memory models, overwritten-source snapshots, input/output/context and stack
  sentinels, signed-halfword promotion, and GPR/FPR/SDA/stack restoration.
  Volatile-pointer cases switch the pointer's referent between its first and
  second reads and require the correct read count and referent for each access.
- **5,120** configured-prefix calls against the original five DOL functions,
  covering every valid TEV stage/table mode, both operation branches, random
  field inputs, complete 1,456-byte GX context, FIFO widths/values, and saved
  GPR/FPR/stack restoration. The original functions execute without call stubs.

The complete configured `GXBump.c` object remains byte-identical, SHA-256
`411f42222cf4814ac1272149fd7319eb67afdfb5413e0d29a42b38e9cfc505e0`;
the checked-in linked-DOL verifier still reports **8/8 exact functions**.
The complete configured AX object also remains byte-identical, SHA-256
`1a8fed48a1634517cd66e23f09754e75274d170ca826e61408ff96650605e7e3`.
The cached paired-memory matrix retains **300/300 exact objects**. The index
panel retains **1,097 unchanged objects / 972 known reference matches /
577 identical declines**. The cumulative metadata panel against `db48e092`
retains **1,494 unchanged objects / 1,179 known matches / 704 identical declines**.
Neither native panel timed out. **3 structured-leaf, 17 switch-lowering,
2 member-array constant-store, 31 frame-convention, and 40 object-writer tests
pass**.

Local scripts are `target/check_member_global_base*.py` and
`target/probe_member_global_base*.py`; artifacts are in
`target/member-global-base-{canaries,tev,gx-library,index,metadata,full-ax}/`.
The initial fourteen-unit survey is pinned in `target/gx-library-baseline/`.
The six existing wibo processes remain in kernel U state after more than
6 hours 47 minutes. No fresh reference-compiler process or full-project parity
panel was launched. Complete project builds and cross-version reference parity
remain open.

## All eight GX functions have exact linked text, 2026-09-07

The configured BfBB **`GXSetIndTexMtx` now matches all 376 original linked
instruction bytes**, improving from 432 bytes under frozen baseline `ec1b8362`.
Its original/candidate linked-code SHA-256 is
`75aa00f582e6d1eaa4b9dbaac91d3b437d530d464b53e959d51d1f058698e174`.
**All eight functions in the complete configured `GXBump.c` object now have
exact linked text**, including the four-byte empty update function. This
compares code at original DOL link addresses, not relocatable object metadata,
debug information, or complete project output. Cross-version reference parity
and complete project builds remain open.

The complete configured GX object SHA-256 is
`411f42222cf4814ac1272149fd7319eb67afdfb5413e0d29a42b38e9cfc505e0`.
Text shrinks **1,248 → 1,192 bytes**; the ELF shrinks **2,792 → 2,376 bytes**.
The matrix dispatch no longer emits a jump table. The other seven function
sizes are unchanged.

The matrix owner now recognizes `__rlwimi` using the shared intrinsic decoder
and reuses its existing range-switch dispatch for both operation kinds.
The intrinsic schedule pairs float loads, multiplies, conversions, and spills;
it preserves r31/r30/r29 in a 112-byte frame and interleaves packet fields
with FIFO writes. Its port, command, and state-flag offset come from the
recognized source. The existing linkage-first profile and a matching
absolute-object declaration select this schedule. Shared body normalization
handles macro wrappers; an assertion no-op is optional. All twelve fields
must have one operation kind, and the measured matrix layout, factor, masks,
selector ranges, and argument types must match.

The recognizer now preserves narrowing casts, requires the actual signed-word
float conversion, and excludes volatile state-pointer globals. These cases
use general emission. Thirty older matrix objects (1779/1780) remain
byte-identical, and all thirty `legacy` C functions in the new corpus retain
their prior instruction bytes and relocation tuples.

Eight new canaries **1821–1828** compile **120/120 objects** on both baseline
and candidate across fifteen builds at O0/O4. They cover the full intrinsic
reduction, wrapped/flat layouts, a different port/command/flag offset, C masks,
narrowed field inputs, narrowed converted values and packet outputs, mixed
operations, volatile pointers, and explicit pointer-cast ports.

Validation totals **115,712 passing candidate calls**:

- **107,520** calls in the new corpus, including **30,720** complete matrix
  reductions compared with original DOL fixtures. Other variants use an
  independent float-conversion/packet model. Inputs cover every signed-byte
  exponent with garbage upper ABI bits, all selector arms and invalid ranges,
  random floats, signed zero, subnormals, and finite conversion boundaries.
  Checks include FIFO address/width/value, context and caller-stack sentinels,
  matrix-input preservation, and stack/SDA/saved GPR/FPR restoration. Volatile
  cases require one pointer read after all six FIFO writes.
  The baseline has **1,071 narrowed-source, 1,071 narrowed-conversion,
  2,048 narrowed-output, and 2,048 volatile-order failures**; the candidate
  has none.
- **8,192** calls rerun all eight complete configured GX functions against the
  original DOL, comparing full context memory, FIFO traces, matrix-input
  preservation, and GPR/FPR/stack restoration.

A reusable checker, `tools/compare_linked_dol.py`, now verifies the linked-text
claim from `docs/reference-layouts/bfbb-gxbump.json`. It pins the original DOL
and symbol-map hashes, validates named data placements against the map and
literal bytes against the DOL, resolves configured SDA/relative-call
relocations, and treats unknown or ambiguous placements as mismatches.
The matrix's 1024.0f constant is verified at original address `0x803cfd18`.
The checker reports **7/8 for the frozen baseline and 8/8 for the candidate**.
To reproduce with a configured GX candidate object:

```sh
python3 tools/compare_linked_dol.py \
  --object target/gx-matrix-gx/full-candidate.o \
  --dol ../Metrowerks/reference_projects/battle_for_bikini_bottom/orig/GQPE78/sys/main.dol \
  --symbols ../Metrowerks/reference_projects/battle_for_bikini_bottom/config/GQPE78/symbols.txt \
  --layout docs/reference-layouts/bfbb-gxbump.json \
  --output target/gx-matrix-gx/verified-linked-text.json
```

Only configured GC/1.2.5n supplies original linked-byte evidence; the other
builds have candidate execution coverage. The cached paired-memory matrix
retains **300/300 exact objects**. The index panel retains **1,097 unchanged
objects / 972 known reference matches / 577 identical declines**. The cumulative
metadata panel against `db48e092` retains **1,494 unchanged objects / 1,179 known
matches / 704 identical declines**. Neither native panel timed out.
**5 loop-normalization, 31 frame-convention, 40 object-writer, and 10 DOL
extraction/comparison tests pass**. The complete configured AX object remains
byte-identical, SHA-256
`1a8fed48a1634517cd66e23f09754e75274d170ca826e61408ff96650605e7e3`.

Local execution/probe scripts are `target/check_gx_matrix*.py` and
`target/probe_gx_matrix*.py`; results, pinned originals, and object hashes are
in `target/gx-matrix-{canaries,gx,index,metadata,legacy,full-ax}/`.
The six existing wibo processes remain in kernel U state after more than
6 hours 31 minutes. No new reference-compiler process was launched and no fresh
full-project panel was measured.

## Exact GX indirect packet setup, 2026-09-07

The configured BfBB **`GXSetTevIndirect` now matches all 108 original linked
instruction bytes**, improving from 124 bytes under frozen baseline `243a1adc`.
Its original/candidate linked-code SHA-256 is
`139a706141f2830879900b9e9cac556eb46cfb88ccc0078feb09898ba422ae61`.
**Seven of the complete GX object's eight functions now have exact linked
text**. Only `GXSetIndTexMtx` remains different, at 432 versus 376 bytes.
Known symbol relocations are resolved at original DOL addresses; this is not
a fresh relocatable reference-object comparison or complete GX parity.

The complete configured GX object remains compilable, SHA-256
`4212aaad54ce3157a8ad228cf5adc83297d3ba5d7b0d53e4796962c8154566e5`.
Text shrinks **1,264 → 1,248 bytes**; the ELF shrinks **2,808 → 2,792 bytes**.
The other seven function sizes are unchanged.

The existing packet owner now distinguishes C clear-mask/shift/OR updates
from `__rlwimi` updates using the shared intrinsic decoder. Its intrinsic
schedule uses a 48-byte frame, preserves r31, loads the ninth byte and tenth
word through the shared EABI stack offsets, and interleaves field inserts
with FIFO setup. The source determines the port address, command, and state
flag offset. The schedule requires the existing linkage-first profile,
a matching absolute-object port declaration, the measured ten-field layout,
and the matching word/byte argument types. Recognition accepts zero-initialized
locals and the shared normalizer's flattened macro bodies. Mixed operations,
narrowing casts, and volatile state-pointer globals use general emission;
the C form retains shifted bits outside each cleared field. Thirty prior
packet objects (1767/1768 across all builds) remain byte-identical.

Local-to-store folding now preserves literal no-op expressions, including
`(void)0`, so disabled assertions no longer stop otherwise valid folding.
The existing global-snapshot-write barrier remains enforced and tested.

Eight new canaries **1813–1820** improve **0/120 → 105/120 compiled objects**
across fifteen builds at O0/O4. They cover the complete indirect reduction,
initialized/flat forms, a different port and flag offset, a different command,
C overflow behavior, narrowed sources/results, mixed operations, volatile
pointers, and explicit pointer-cast ports. The optimized C-mask translation
unit still declines in `narrow_source` because allocated callee-saved values
need a canonical frame owner; its O0 counterpart compiles on every build.
This remaining frame limitation is retained in the corpus.

Validation totals **92,672 passing candidate calls**:

- **84,480** calls in the compiled new corpus, including **30,720** complete
  indirect reductions compared with original DOL context/FIFO fixtures.
  Checks cover FIFO address/width/value, context and caller-stack sentinels,
  all byte argument values with garbage upper ABI bits, and stack/SDA/saved
  GPR/FPR restoration. Volatile cases enforce one pointer read after both
  FIFO writes. Other variants use an independent C/intrinsic packet model.
- **8,192** calls rerun all eight complete configured GX functions against
  the original DOL, comparing FIFO traces, complete context memory, matrix
  input preservation, and GPR/FPR/stack restoration.

Only configured GC/1.2.5n supplies original linked-byte evidence; the other
builds have candidate execution coverage. The captured paired-memory matrix
retains **300/300 exact objects**. The index panel retains **1,097 unchanged
objects / 972 known reference matches / 577 identical declines**. The cumulative
metadata panel against `db48e092` retains **1,494 unchanged objects / 1,179 known
matches / 704 identical declines**. Neither native panel timed out.
**1 local-folding, 3 incoming-parameter, 31 frame-convention, 5 loop-normalization,
and 40 object-writer tests pass**. The complete configured AX object remains
byte-identical, SHA-256
`1a8fed48a1634517cd66e23f09754e75274d170ca826e61408ff96650605e7e3`.

Local scripts are `target/check_gx_packet*.py` and `target/probe_gx_packet*.py`;
results, pinned originals, and object hashes are in
`target/gx-packet-{canaries,gx,index,metadata,legacy,full-ax}/`.
The six existing wibo processes remain in kernel U state after more than
6 hours 18 minutes. No new reference-compiler process was launched and no fresh
full-project panel was measured.

## Exact GX coordinate scaling and virtual bitwise subtrees, 2026-09-07

The configured BfBB **`GXSetIndTexCoordScale` now matches all 324 original linked
instruction bytes**, improving from 472 bytes under frozen baseline `169d655e`.
Its original/candidate linked-code SHA-256 is
`806457e70dbe416723c7521fe0e89dd62400df5d37cf1fff2e3eb6d87f4f60d7`.
**Six of the complete GX object's eight functions now have exact linked text**.
Indirect setup and matrix setup remain different. Known symbol relocations
are resolved at original DOL addresses; this is not a fresh relocatable
reference-object comparison, and complete GX parity remains open.

The complete configured GX object remains compilable, SHA-256
`191dac12b5e2499b7ab48b68ef43e312436d2f8876bd4399d41cd9cbb014ab7f`.
Text shrinks **1,412 → 1,264 bytes**; the ELF shrinks **3,240 → 2,808 bytes**.
The other seven function sizes are unchanged.

The existing scale-switch owner now distinguishes C clear-mask/shift/OR updates
from `__rlwimi` updates. Both share the dispatch and a plan derived from the
source's state-member offsets. The C form retains shifted source bits outside
the cleared field and keeps its prior schedule. The intrinsic form inserts
only the specified mask, retains one state pointer per arm, and interleaves
the command/address materialization with the field updates. Its schedule
requires the existing linkage-first profile and a matching absolute-object
port declaration. Recognition accepts wrapped and flat statements and an
optional macro no-op, but requires all three updates in all four arms to have
the same operation kind. Mixed forms use general emission.

Recognition now preserves narrowing casts and excludes volatile state-pointer
globals. The old C matcher stripped an unsigned-byte source cast and suppressed
volatile pointer reads. The safe fallback for the narrowed mask expression
exposed the general emitter's single-scratch limit. Pure `&`, `|`, `^`, and `<<`
expressions that exceed it now materialize the two subtrees into independent
virtual GPRs after the specialized schedules have declined. They preserve
pending input registers and prefer an immediate instruction for a constant
left shift. This does not complete arbitrary arithmetic or call-bearing trees.

Ten new canaries **1803–1812** improve **98/150 → 150/150 compiled objects**
across fifteen builds at O0/O4. They cover the complete scale reduction with
absolute-object syntax, compact and aliased state layouts, wrapped/flat forms,
legacy C overflow behavior, narrowing casts, mixed C/intrinsic updates,
volatile pointers, and bitwise subtrees with signed/unsigned narrowing,
constant/dynamic shifts, stored results, and repeated input variables.
The eight available baseline `legacy` functions retain identical instruction
bytes and relocation tuples after the change.

Validation totals **154,112 passing candidate calls**:

- **145,920** calls in the new corpus, including **30,720** complete scale
  reductions compared with original DOL context/FIFO fixtures. Checks cover
  full state memory, output-memory sentinels, FIFO width/value, return values,
  stack/SDA, and saved GPR/FPR restoration. The volatile cases enforce eight
  state-pointer reads for a selected arm and one for the default path.
  The baseline executes 75,264 calls with **434 narrowing failures** and
  **1,024 volatile-read-count failures**; the candidate has none.
- **8,192** calls rerun all eight complete configured GX functions against the
  original DOL, comparing FIFO traces, complete context memory, matrix input
  preservation, and GPR/FPR/stack restoration.

Only the configured GC/1.2.5n original supplies linked-byte evidence; the other
builds have candidate execution coverage. The captured paired-memory matrix
retains **300/300 exact objects**. The index panel retains **1,097 unchanged
objects / 972 known reference matches / 577 identical declines**. The cumulative
metadata panel against `db48e092` retains **1,494 unchanged objects / 1,179 known
matches / 704 identical declines**. Neither native panel timed out.
**4 intrinsic, 5 loop-normalization, and 40 object-writer tests pass**.
The complete configured AX object remains byte-identical, SHA-256
`1a8fed48a1634517cd66e23f09754e75274d170ca826e61408ff96650605e7e3`.

Local scripts are `target/check_gx_scale*.py` and `target/probe_gx_scale*.py`;
results, pinned originals, and object hashes are in
`target/gx-scale-{canaries,gx,index,metadata,full-ax}/`.
The six existing wibo processes remain in kernel U state after more than
5 hours 58 minutes. No new reference-compiler process was launched and no fresh
full-project panel was measured.

## Exact GX order update and terminal port flushes, 2026-09-07

The configured BfBB **`GXSetIndTexOrder` now matches all 236 original linked
instruction bytes**, improving from 248 bytes under frozen baseline `6eb6ba53`.
Its original/candidate linked-code SHA-256 is
`b41a2361c65528a180ae0cfd4fb20ff3d1bed1758ab5f40764ff63849d5fd6cd`.
**Five of the complete GX object's eight functions now have exact linked text**;
the other four retained matches are `GXSetNumIndStages`, `GXSetTevDirect`,
`__GXFlushTextureState`, and the empty `__GXUpdateBPMask`. Known symbol
relocations are resolved at original DOL addresses. This is not a fresh
relocatable reference-object comparison.

The complete configured GX object remains compilable, SHA-256
`0c3316db0b75041d68c0c3e2e50547132b37a27ebc24d432a430555c3e97f697`.
Text shrinks **1,424 → 1,412 bytes**; the ELF shrinks **3,296 → 3,240 bytes**.
The other seven function sizes are unchanged. Indirect setup, matrix setup,
and coordinate scaling still differ from the original; full GX parity is open.

The existing absolute-object port-flush owner now separates recognition from
emission. Its plan represents a command/data pair, optional dirty-word OR,
and halfword clear. A whole function and a terminal structured region share
one emitter. The structured lowerer emits guards and switch arms normally,
then the eligible build profile retains one state pointer through the tail.
Early returns skip the complete tail. All profiles can lower this control flow;
the linkage-first profile alone selects the measured retained-pointer schedule.
Word casts from SDK macros are accepted, while narrowing casts, aggregate
object bases, volatile pointer globals, different state bases, and unsupported
masks retain general emission. The declaration-address metadata continues to
select absolute-object scheduling.

Two correctness issues surfaced in the boundary corpus. The old three-store
flush owner reused a volatile pointer, suppressing its second read and clearing
the wrong object when the pointer changed. Recognition now excludes volatile
pointer globals. It also distinguishes aggregate storage from a pointer value,
so an ordinary struct global reaches address-based emission. Exercising that
fallback exposed a fixed-bank scheduler swapping a symbol-address instruction
without its relocation. The fixed-bank pass now excludes relocation/deferred-
displacement owners and uses the shared instruction-move operation to preserve
metadata and branch destinations for accepted literal-address schedules.

Ten new canaries **1793–1802** improve **112/150 → 150/150 compiled objects**
across fifteen builds at O0/O4. The gains are thirty early-return wrapper
objects and eight aggregate-object cases. They cover ordinary/dirty/aliased
flushes, branch updates, pointer replacement, early returns, the complete GX
order reduction with absolute-object syntax, volatile pointers, differing
state bases, high masks, an explicit port cast at a different address,
narrowing data casts, aggregate storage, and an overlapping halfword clear.

Validation totals **146,432 passing candidate calls**:

- **138,240** calls cover the new corpus, including **30,720** order-reduction
  calls compared with the original DOL's complete context and FIFO fixtures.
  Checks include both state objects, pointer replacement, FIFO address/width/
  value, stack/SDA, and saved GPR/FPR restoration. Aggregate addresses exercise
  positive and negative low-half relocation values. The baseline executes
  109,056 calls with 2,048 failures, all from suppressed volatile-pointer
  reloads in the simple flush.
- **8,192** calls rerun all eight complete configured GX functions against
  the original DOL, comparing FIFO traces, complete context memory, matrix
  input preservation, and GPR/FPR/stack restoration.

The volatile compound-update boundary still emits four pointer reads in both
baseline and candidate. Its fixture changes the pointer after the first read
and then holds it stable; it does not establish full volatile compound-update
read-count parity. Only the configured GC/1.2.5n original supplies linked-byte
evidence; the other builds have candidate execution coverage.

The captured paired-memory matrix retains **300/300 exact objects**. The index
panel retains **1,097 unchanged objects / 972 known reference matches / 577
identical declines**. The cumulative metadata panel against `db48e092` retains
**1,494 unchanged objects / 1,179 known matches / 704 identical declines**.
Neither native panel timed out. **3 structured-leaf, 17 switch-lowering,
1 fixed-bank scheduling, 11 instruction-index, and 40 object-writer tests pass**.
The bank test now rejects symbol/deferred address operands while retaining
literal bank scheduling. The complete configured AX object remains byte-identical,
SHA-256 `1a8fed48a1634517cd66e23f09754e75274d170ca826e61408ff96650605e7e3`.

Local scripts are `target/check_gx_flush*.py` and `target/probe_gx_flush*.py`;
results, pinned originals, and object hashes are in
`target/gx-flush-{canaries,gx,index,metadata,full-ax}/`.
The six existing wibo processes remain in kernel U state after more than
5 hours 41 minutes. No new reference-compiler process was launched and no fresh
full-project panel was measured.

## Exact GX forwarding and outgoing stack words, 2026-09-07

The configured BfBB **`GXSetTevDirect` now matches all 72 original linked
instruction bytes**, improving from 76 bytes under frozen baseline `fa1bdc84`.
Its original/candidate linked-code SHA-256 is
`331083a14facf477a788303f0b97fcfa83689dded3bf4678e9e7d176778771f0`.
Together with `GXSetNumIndStages`, `__GXFlushTextureState`, and the empty
`__GXUpdateBPMask`, **four of the complete GX object's eight functions have
exact linked text**. Known symbol relocations are resolved at original DOL
addresses; this is not a fresh relocatable reference-object comparison.
The complete configured object remains compilable, SHA-256
`575451c7cdab3fb8d7efc4123f4c704eb907b4f7abbe30724a1d099d92f48d6c`.
Text shrinks **1,428 → 1,424 bytes**; the ELF remains 3,296 bytes. The other
seven function sizes are unchanged. Full GX instruction/object parity is open.

The existing linkage-first zero-forwarding scheduler now accepts a plain call
as well as a call preceded by a macro no-op. It verifies that the callee is a
direct call with ten word-class positions and that the passthrough argument
needs no narrowing. Floating, wide, reference, aggregate, and indirect calls
fall back to general emission. This retains the existing build-profile gate.
The new prototype checks also fix incorrect floating/narrowing macro wrappers
that the old scheduler accepted.

General outgoing word arguments now use virtual temporaries instead of treating
the logical argument cursor beyond r10 as a physical register number. This
prevents eleventh and later arguments from writing SDA/callee-saved registers.
Already placed r3–r10 arguments are reserved during materialization, including
allocation constraints on newly created nested address/value temporaries.
Structured frames reserve outgoing word capacity before locating saved homes.
Existing checks still reject overlap with local slots: this change does not
relocate locals, or complete every wide/narrow/call-containing argument path.

Eight new canaries **1785–1792** improve **60/120 → 120/120 compiled objects**
across fifteen builds at O0/O4. They cover ten-zero wrappers, eleven arguments
with literal/computed/loaded stack words, sixteen arguments with large words,
and floating/narrowing prototypes. Computed arithmetic is unsigned so the
wraparound fixtures have defined behavior. **76,800 candidate calls pass**, with
argument values and stack/SDA/callee-saved GPR/FPR restoration checked. The
baseline executes 46,080 calls, of which 4,080 expose the incorrect floating or
narrowing macro wrappers; it declines the 60 eleven/sixteen-argument objects.
Instruction-byte evidence is from the configured GC/1.2.5n original; the other
builds have candidate execution coverage.

Validation totals **106,112 passing candidate calls**:

- **76,800** in the new corpus described above.
- **21,120** in the prior incoming-stack corpus, including ten-word forwarding,
  call survival, pointer arguments, and address-taken parameters. All 270 prior
  incoming-stack objects still compile; only the 30 forwarding objects change.
- **8,192** across all eight complete GX functions against the original DOL,
  comparing FIFO traces, complete context memory, matrix input preservation,
  and GPR/FPR/stack restoration.

The captured paired-memory matrix retains **300/300 exact objects**. The index
panel retains **1,097 unchanged objects / 972 known reference matches / 577
identical declines**. The cumulative metadata panel against `db48e092` retains
**1,494 unchanged objects / 1,179 known matches / 704 identical declines**.
Neither native panel timed out. **31 frame-convention, 3 incoming-parameter,
5 vreg-frame, and 40 object-writer tests pass**. The complete configured AX
object remains byte-identical, SHA-256
`1a8fed48a1634517cd66e23f09754e75274d170ca826e61408ff96650605e7e3`.
Removing a redundant behavior flag during review preserves all 392 measured
new/prior canary and complete-project object hashes.

Local scripts are `target/check_gx_forward*.py` and `target/probe_gx_forward*.py`;
results, pinned originals, and object hashes are in
`target/gx-forward-{canaries,incoming-canaries,gx,index,metadata,full-ax}/`.
The six existing wibo processes remain in kernel U state after more than
5 hours 21 minutes. No new reference-compiler process was launched and no fresh
full-project panel was measured.

## Exact GX stage update and address-base constraints, 2026-09-07

The configured BfBB **`GXSetNumIndStages` now matches all 36 original linked
instruction bytes**, improving from 52 bytes under frozen baseline `2912b25e`.
The original/candidate linked-code SHA-256 is
`d4be2a9960dc8bb6cb8562457cce7a9d02c431561989fd43466da4ec22266ea6`.
Together with `__GXFlushTextureState` and the empty `__GXUpdateBPMask`,
**three of the complete GX object's eight functions have exact linked text**.
This comparison resolves known symbol relocations at original DOL addresses;
it is not a fresh relocatable reference-object comparison.

The complete configured `GXBump.c` object remains compilable. Its SHA-256 is
`ec3f0fcda452a7119d86167dbd7ff4016649fb653c5aece40d335b83bf9cb336`.
Text shrinks **1,476 → 1,428 bytes**; the ELF file shrinks **3,376 → 3,296 bytes**.
`GXSetIndTexOrder` also shrinks **280 → 248 bytes**, with redundant insert-result
copies removed; its original is still 236 bytes. The other five function sizes
remain unchanged. Full instruction and whole-object parity remain open.

The existing global-field/dirty-mask scheduler now distinguishes two source
operations explicitly. A C clear-mask/shift/OR expression inserts the entire
shifted byte, including bits outside the cleared field. An `__rlwimi` inserts
only its specified mask. Both retain their own schedules. The intrinsic form
loads the nonvolatile global pointer once, narrows the byte parameter, updates
the selected field, then ORs the dirty word. It handles parser-retained compound
update values and optional macro no-ops, while preserving narrowing casts and
excluding volatile global pointers from pointer-load reuse. Immediate operands
are validated by a shared intrinsic decoder used by both general selection
and body scheduling.

General rotate-insert results prefer r0 when their consumer already wants the
scratch result. Their separate virtual identities still preserve operand
aliasing. Execution testing exposed why this preference needed a machine
constraint: a virtual used to form an address could otherwise color into r0,
turning its address use into literal zero. The machine description now identifies
zero-sensitive base fields, liveness forbids r0 for those general virtuals,
and selection hints are merged with those restrictions instead of overwriting
them. Indexed offset operands remain eligible for r0; floating-register values
do not inherit the general-register restriction.

Four new canaries **1781–1784** produce **60/60 objects** across fifteen builds
at O0/O4, both before and after the change. They cover narrow/full/wrapping
insert masks, dirty-word aliasing, the distinct C shift/OR behavior, narrowed
old values/results, volatile pointer loads, and the full stage-count reduction.
This is a correctness and instruction-parity milestone, not a compile-count
gain. Only the configured GC/1.2.5n original supplies instruction-byte evidence;
other builds have candidate execution coverage.

At the final fingerprint, **215,552 candidate execution calls pass**:

- **92,160** calls for the new corpus objects, including every byte input with
  randomized unused register bits. Baseline executions also pass. Volatile
  pointer cases retain all four pointer loads per call under both compilers.
  Full stage-count reductions compare with pinned original-DOL fixtures.
- **115,200** calls rerun canaries 1771–1780 under the new compiler, retaining
  the switch, matrix, pointer-continuation, and ABI results. Their thirty
  baseline matrix objects also pass all 30,720 checks.
- **8,192** calls rerun all eight functions in the complete configured GX
  object against the original DOL, comparing FIFO traces, complete context
  memory, matrix input preservation, and GPR/FPR/stack restoration.

Other validation:

- Captured paired-memory matrix: **300/300 whole-object exact**.
- Index panel: **1,097 objects unchanged**, retaining **972 known reference
  matches**, and **577 identical declines**, with no timeouts.
- Cumulative metadata panel against `db48e092`: **1,494 objects unchanged**,
  retaining **1,179 known reference matches**, and **704 identical declines**,
  with no timeouts.
- **102 vreg tests pass** with eight pre-existing ignored tests; **4 intrinsic
  tests and 40 object-writer tests pass**. New tests enforce address restrictions
  despite an r0 preference, retain r0 for indexed offsets, and validate immediate
  limits without rejecting wrapping masks.
- The complete configured AX object remains byte-identical, SHA-256
  `1a8fed48a1634517cd66e23f09754e75274d170ca826e61408ff96650605e7e3`,
  retaining its prior complete sync execution evidence.

Local scripts are `target/check_gx_insert*.py` and `target/probe_gx_insert*.py`;
results, pinned originals, and object hashes are in
`target/gx-insert-{canaries,prior-canaries,gx,index,metadata,full-ax}/`.
The six existing wibo processes remain in kernel U state after more than five
hours. No new reference-compiler process was launched and no fresh full-project
panel was measured.

## Complete GXBump compilation and switch execution, 2026-09-07

The **complete configured BfBB `GXBump.c` now compiles** under GC/1.2.5n,
using its DolphinLib flags and unchanged project source/headers. Frozen
baseline `b0d530ae` declines in `GXSetIndTexCoordScale`. The complete candidate
object SHA-256 is
`0985d613f3b8774b41354d9302eb6a1feb955b005b9d0bd03c5280d35ed397b8`;
three independent recompilations produce identical objects.

The shared constant-false `do`-loop normalizer now descends into switch cases
and defaults, preserving case values, order, and fallthrough. Control-edge
analysis distinguishes switch-local breaks from continues targeting the
surrounding loop; wrappers containing their own early exits remain intact.

Leaf functions with guarded assignments, switch-selected locals, or joined
fallthrough arms now use the existing structured switch and named-value
lowering. Analysis and emission use the same statement tree, including
restored terminal guards: cloning a separate emission list previously lost
statement-identity liveness facts. Existing simple terminal-switch owners
retain their dispatch policy.

Two execution failures exposed by full-source compilation are fixed:

- Generic statement-switch arms now share the structured emitter's cache reset
  at case/default entries and the join. A FIFO base materialized in one arm
  can no longer be reused by another arm that never executed that definition.
- Dense dispatch preserves physical argument homes read by the arms **or the
  continuation** before its r3/r4 scratch use. The matrix input pointer had been
  overwritten by the jump-table address. Retained homes are ordered by physical
  register/name so generated code is independent of hash-map iteration.

Ten new canaries **1771–1780** cover switch macro wrappers, nested/default and
fallthrough paths, GX scale/order updates, a dense-switch pointer continuation,
and the complete matrix-packet reduction. **150/150 objects compile** across
fifteen builds at O0/O4, up from **30/150** under the frozen baseline.
The thirty matrix objects already compiled before the change; their execution
improves from **23,040 failures in 30,720 calls to zero failures**.

At the final compiler fingerprint, **123,392 candidate executions pass**:

- **8,192** calls exercise all eight functions in the complete configured GX
  object against their original GQPE78 DOL implementations, with 1,024 fixtures
  per function. The emulator loads the original DOL's code/data, including the
  real matrix constants and internal `GXSetTevIndirect` call. Checks compare
  ordered FIFO writes, all 1,456 context bytes, unchanged matrix input, stack
  restoration, and callee-saved GPR/FPRs. Fixtures cover valid/out-of-range
  selectors, 255-to-zero parameter guards, randomized field values, all byte
  scale exponents, and finite matrix entries in [-2, 2].
- **115,200** calls execute the 150 corpus objects. GX reductions compare with
  the same pinned original fixtures; standalone switch cases check expected
  state updates or indexed return values and ABI preservation.

Linked-text comparison applies the known SDA and direct-call relocations at
original symbol addresses. **`__GXFlushTextureState` (36 bytes) and
`__GXUpdateBPMask` (4 bytes) are exact**, including the empty function.
The other six functions still differ in size:

| Function | Candidate bytes | Original bytes |
| --- | ---: | ---: |
| `GXSetTevIndirect` | 124 | 108 |
| `GXSetIndTexMtx` | 432 | 376 |
| `GXSetIndTexCoordScale` | 472 | 324 |
| `GXSetIndTexOrder` | 280 | 236 |
| `GXSetNumIndStages` | 52 | 36 |
| `GXSetTevDirect` | 76 | 72 |

The matrix function's anonymous table/constant relocations remain unresolved
in that text comparison; its differing length already excludes exactness.
This milestone proves tested execution equivalence and compilation of this
translation unit, not whole-object or full-project parity. Remaining GX work
includes instruction selection, register placement, and scheduling for those
six functions.

Other validation:

- Captured paired-memory matrix: **300/300 whole-object exact**.
- Index panel: **1,097 objects unchanged**, retaining **972 known reference
  matches**, and **577 identical declines**, with no timeouts.
- Cumulative metadata panel against `db48e092`: **1,494 objects unchanged**,
  retaining **1,179 known reference matches**, and **704 identical declines**,
  with no timeouts.
- **44 targeted codegen tests and 40 object-writer tests pass**: loop
  normalization, leaf eligibility, dense dispatch, switch lowering, and named
  value flow.
- The complete configured AX object remains byte-identical, SHA-256
  `1a8fed48a1634517cd66e23f09754e75274d170ca826e61408ff96650605e7e3`,
  retaining its prior complete sync execution evidence.

Local scripts are `target/check_gx_switch*.py` and
`target/probe_gx_switch*.py`; objects, input manifests, fixtures, and results
are under `target/gx-switch-{canaries,gx,index,metadata,full-ax}/`.
Original-function manifests pin the same DOL/map hashes as earlier GX work.
The six existing wibo processes remain in kernel U state after more than
four hours forty-five minutes. No new reference compiler was launched and
no fresh reference-object or full-project-panel gain is claimed.

## GX FIFO stores and rotate-insert execution, 2026-09-07

The unmodified source prefix containing BfBB's complete `GXSetTevIndirect`
now compiles with its configured GC/1.2.5n DolphinLib flags. The frozen
`771fce48` compiler declined its first constant-address FIFO member store.
The candidate emits **124 bytes**, compared with **108 bytes** in the original
GQPE78 DOL. This is full isolated-function execution coverage, not instruction
parity or complete `GXBump.c` compilation.

Fresh constant-address bases now support nonzero literal and computed store
values, including floating values. Value computation precedes a fresh base;
the existing register-leaf and zero-store schedules remain intact. Existing
base reuse still reserves the base across value computation. Unsupported
address regions and frame shapes continue to decline.

`__rlwimi` has a shared five-argument intrinsic identity used by call analysis,
register-pressure analysis, symbol traversal, and both expression and direct
call emission. It emits the PowerPC rotate-and-mask-insert instruction with
constant shift/mask operands in 0..31. Separate virtual identities preserve
the old destination and source when the result aliases an input or inputs are
nested expressions. Operand effects remain visible to call/effect analysis.
Nonconstant or out-of-range immediate operands decline; their reference
compiler diagnostics have not been measured.

Eight new canaries **1763–1770** capture literal, computed, callback-returned,
float/double FIFO stores; full and wrapping masks, aliased and nested intrinsic
values; a complete GX FIFO reduction with ninth/tenth stack arguments; and
remaining call-spanning frame limitations. **90/120 objects compile**, up from
**0/120** under the frozen baseline, across fifteen builds at O0/O4. The thirty
objects for 1769–1770 still decline because allocated values need a canonical
frame owner.

At the final fingerprint, **118,272 candidate executions pass**:

- **31,744** complete GX function calls: thirty reduction objects plus the
  configured source-prefix object, each checked against 1,024 executions of
  the original linked function. Checks compare ordered FIFO command/word
  writes, the full context image, stack restoration, and callee-saved GPRs.
  The original code hash remains
  `139a706141f2830879900b9e9cac556eb46cfb88ccc0078feb09898ba422ae61`.
- **84,480** store and intrinsic calls over sixty objects, checking integer
  wraparound, wrapping masks, aliases, floating store bits, callback counts,
  volatile-register clobbering, and ABI preservation.
- **2,048** calls for newly compiling GC/1.2.5n canary **1274**, checking both
  state-word update functions, full state memory, FIFO writes, and ABI state.
  Execution first exposed a scratch register overwriting a later parameter.
  The straight-line body now shares the structured body's existing reservation
  helper for physical homes read by later statements, correcting that failure.

The complete configured `GXBump.c` progresses to a loop-lowering decline in
`GXSetIndTexCoordScale`; its switch contains `do { ... } while (0)` macros.
The current shared normalizer does not recurse into switch arms. That is the
next area to investigate. Reference-project files remain unchanged.

Other validation:

- Captured paired-memory matrix: **300/300 whole-object exact**.
- Index panel: **1,096 prior objects unchanged**, retaining **972 known
  reference matches**; **577 identical declines**; one newly compiled object
  (1274) with execution coverage above; no timeouts.
- Cumulative metadata panel against `db48e092`: **1,494 objects unchanged**,
  retaining **1,179 known reference matches**, and **704 identical declines**;
  no timeouts.
- **3 intrinsic analysis tests and 40 object-writer tests pass**.
- The complete configured AX object remains byte-identical, SHA-256
  `1a8fed48a1634517cd66e23f09754e75274d170ca826e61408ff96650605e7e3`,
  retaining its prior complete sync execution evidence.

Local proof scripts are `target/check_gx_fifo*.py` and
`target/probe_gx_fifo*.py`; results and object hashes are under
`target/gx-fifo-{canaries,gx,index,metadata,full-ax}/` and
`target/memory-operands-probes/gx-fifo-results.json`. The six existing wibo
processes remain in kernel U state after more than four hours twenty minutes.
No new reference-compiler processes were launched; no fresh reference-object
or full-project-panel gain is claimed.

## Incoming stack arguments and GX packet execution, 2026-09-07

The ninth-parameter calling-convention failures in **1743–1744** are fixed.
All thirty objects now pass 1,920 execution calls using the caller's stack
arguments instead of r11/r12. The source cases already compiled under frozen
baseline `e1b66bec`; this milestone corrects their behavior rather than claiming
a compilation-count gain.

Incoming word-class parameters beyond r3–r10 now receive virtual identities.
The selected entry block materializes each needed value before its first use
or control edge. A straight-line definition that kills the incoming value
suppresses its load. Register inputs still referenced by the source are excluded
from these load allocations, including arguments forwarded naturally to a call
without explicit self-moves. Address-taken parameter slots use the same entry
locations. Byte and halfword stack arguments load from the right-aligned part
of their four-byte slot, with the appropriate sign or zero extension.

The machine representation's existing data displacement records have been
generalized to **deferred displacements**, adding a caller-SP target alongside
section symbols and anonymous data. Existing scheduling, insertion, removal,
and relocation remappers carry all three target kinds. Incoming displacements
are resolved from the final stack position and consumed before object assembly;
section displacements retain their existing object-layout resolution. This
avoids baking a provisional frame size into incoming loads. Most touched
scheduler files contain only this shared record/type rename.

Caller and callee now share the EABI word-stack offset calculation. Known
word-only call signatures reserve their outgoing parameter area in plain frame
planning, and linkage-first normalization preserves that minimum. This fixes a
forwarding case whose old eight-byte frame placed its tenth outgoing argument
on the saved LR slot. Stores also reject overlap with declared GPR saves or
locals. Wide-pair incoming stack placement and general floating/variadic
outgoing stack planning remain outside this change.

Sixteen new canaries **1747–1762** cover leaf reads, ninth/tenth parameter sums,
assignments and guards, calls, forwarding, pointer parameters, address-taking,
independent floating-register arguments, narrow stack values, and a BfBB GX
packet reduction. Together with 1743–1744, **270/270 objects compile** across
fifteen builds at O0/O4, both before and after the change. All **84,480 candidate
execution calls pass**:

- AX frame case: 1,920 calls, including packet memory and register preservation.
- Leaf/control/call/pointer/address cases: 21,120 calls, checking forwarded ABI
  registers and stack slots, callback mutation, overflow, and callee-saved state.
- Narrow arguments: 30,720 calls with randomized unused stack-slot bytes,
  exhaustive byte inputs, and signed/unsigned halfword values.
- GX packet reduction: 30,720 comparisons with the original linked function.

The GX source is the configured GC/1.2.5n BfBB `GXBump.c` function
`GXSetTevIndirect`, with ten integer-class parameters. The original 108-byte
function at `0x801CE8CC` is extracted from the pinned GQPE78 DOL/map. Its code
SHA-256 is `139a706141f2830879900b9e9cac556eb46cfb88ccc0078feb09898ba422ae61`.
All **1,024 original executions** check its FIFO command/value writes, context
flag clear, and stack/register restoration. Each of the thirty reduction
objects returns the same packed FIFO value. The reduction expresses the
original constant-mask `__rlwimi` operations in C and returns the packet instead
of writing hardware; it is not full-function or instruction parity.

The full configured `GXBump.c` and an isolated source prefix still decline at
GX's constant-address FIFO member store requiring base reuse. That is the next
real-source blocker. Reference project files were not changed.

Validation at the final compiler fingerprint:

- Captured paired-memory matrix: **300/300 whole-object exact**.
- Index panel: **1,096 objects unchanged**, preserving **972 known reference
  matches**, and **578 identical declines**, with no timeouts.
- Cumulative metadata panel against `db48e092`: **1,494 objects unchanged**,
  preserving **1,179 known reference matches**, and **704 identical declines**,
  with no timeouts.
- **3 incoming-parameter tests, 31 frame-convention tests, and 40 object-writer
  tests pass**.
- The complete configured BfBB AX object remains byte-identical to the previous
  measured object, retaining its complete sync execution evidence.

Local scripts and measurements are in `target/check_incoming*.py`,
`target/probe_incoming_gx.py`, `target/probe_incoming_arguments_full_ax.py`,
`target/incoming-arguments-canaries/`, and `target/incoming-arguments-gx/`.
The six existing wibo processes remain in kernel U state after more than four
hours; no additional reference-compiler processes were launched and no fresh
reference-object or full-project-panel gain is claimed.

## Allocated GPR frame growth and LR restoration, 2026-09-07

The frame failures retained by canaries **1739–1740** are fixed. They now
compile and execute on all fifteen builds at O0/O4, improving **19/30 → 30/30
compilations** and fixing the GC/1.1p1 O4 return-address corruption.

The linkage-first slot relayout now distinguishes a restored-stack LR reload
from saved GPR slots. During frame growth, a logical saved slot can temporarily
occupy offset 4; a later `lwz r0,4(r1)` after stack restoration uses the caller's
SP and must not be repainted as that saved register. The fix covers every
canonical restored-stack reload pair, with a unit regression for the collision.

A separate allocation-frame growth helper now increases canonical predecrement
frames when the allocator requires more individual GPR saves than selection
reserved. It adds an aligned save area above existing locals and outgoing
arguments, moves saved-register slots and caller-frame accesses, and updates
frame metadata. The existing reconciler still owns insertion of physical saves
and restores and branch/relocation retargeting. Untracked stack addressing is
rejected before publishing a resized stream; existing FPR-save layouts retain
their separate owner. Unit coverage checks local versus caller-frame offsets,
address formation, linkage/save offsets, and transactional rejection.

Six new canaries **1741–1746** cover local arrays surviving calls, a ninth
integer parameter, and conditional assignment chains. Across **1739–1746**,
candidate compilation improves **87/120 → 120/120** against frozen `c3625e24`.
Of 87 previously compiling objects, 85 remain byte-identical; the two changes
are GC/1.1p1 O4 LR-reload repairs in 1739 and 1743.

The **90 objects excluding the ninth-parameter cases pass 5,760 execution
calls**, covering every packet index 0–63, complete packet memory, local array
contents across two callbacks, branch outcomes, unsigned return arithmetic,
caller-parameter storage, stack restoration, and all callee-saved GPRs.
Callbacks clobber r3–r12. This includes all thirty original failing-frame probes.

Canaries **1743–1744** expose a distinct incoming-argument bug and are retained
as the next regression target. All thirty compiled objects incorrectly read the
ninth integer parameter from r11. With 1 at the caller's `8(r1)` and 2 in r11,
they return 2. Their compilation gain is not execution parity. The growth
helper's caller-frame displacement handling has unit coverage, but these
frontend/entry-location failures prevent validating that path through these
C functions yet.

Validation at the recorded final fingerprint:

- Captured paired-memory matrix: **300/300 whole-object exact**.
- Index panel: **1,096 compiled objects unchanged**, retaining **972 known
  reference matches**, plus **578 identical declines**; no timeouts.
- Cumulative metadata panel against `db48e092`: **1,494 compiled objects
  unchanged**, retaining **1,179 known reference matches**, plus **704 identical
  declines**; no timeouts.
- **3 allocation-frame tests and 31 frame-convention tests pass**.
- The complete configured BfBB AX object is byte-identical to `c3625e24`'s
  measured object, retaining its 256 complete sync execution comparisons.

Scripts and results are in `target/check_frame_growth*.py`,
`target/probe_frame_growth_full_ax.py`, `target/frame-growth-canaries/`, and
`target/frame-growth-full-ax/`. The existing six wibo processes remain stuck
in kernel U state; no new reference-compiler processes were launched, and no
fresh reference-object or full-project-panel improvement is claimed.

## Complete BfBB AX compilation and sync execution, 2026-09-07

The complete configured BfBB `src/dolphin/src/ax/AXVPB.c` now compiles with
GC/1.2.5n and the project's exact flags and include paths. Its 804-byte
`__AXSyncPBs` passes **256 complete-function execution comparisons** against
the original game's 632-byte function. This is execution evidence with modeled
external helpers, not instruction/object parity or complete-project parity.
The candidate service function remains 2,840 bytes; its instruction bytes are
unchanged from the previously measured isolated service. Reference project
files were not edited.

The lookup-sum planner now accepts member-derived masked indices. It shares an
index load within one optimized sum only when source facts positively establish
an ordinary, nonvolatile pointee. The existing source-fact set now includes
local pointer declarations and their block shadow names as well as parameters.
Volatile fields, volatile-qualified pointees, and unknown bindings retain
separate reads. O0 keeps its three index reads; a call between sums starts a
new sharing scope. Table loads themselves are never shared by this change.
The two-load selector retains its existing resident-index restriction.

The final inactive-voice sweep now uses the existing multiply-based global
array element-address routine for member stores with non-power-of-two strides.
This handles AX's 244-byte packet elements and chained halfword clears without
adding a separate scaling implementation.

Complete sync execution caught a section-anchor lifetime bug: a deferred inline
parameter reused the table-base register after its textually last use inside
the loop, corrupting table reads on subsequent iterations. Anchor uses now
extend through the containing loop's end, including nested loops and condition
uses. Reuse after the loop remains possible. Before the fix, 103/256 sync cases
had wrong cycle totals despite identical memory updates and callback traces;
all 256 now match. Two unit regressions cover nested backedges, loop conditions,
and preservation of safe straight-line reuse.

The full sync comparison executes both candidate and original service callees.
It varies voice counts 0/1/2/7/16/64, priority lists, cycle limits and unsigned
wraparound, sync flags, DSP states, update counts, and randomized storage.
It compares the entire 0x11800-byte AX BSS allocation, all three counters,
callback-stack head, and helper trace. Cache helpers are modeled as no-ops;
command-cycle and priority-head helpers use controlled fixture inputs; depop
and callback-stack helpers apply deterministic memory effects. All helpers
clobber caller-saved registers. Return, stack restoration, and callee-saved
registers are checked. The original functions and tables come from the pinned
GQPE78 DOL/map already recorded below.

Canaries **1727–1738 improve 0/180 → 180/180 compilations** against frozen
baseline `5ae8188e`, covering all fifteen builds at O0/O4. The member lookup
samples pass **1,981,440 candidate calls**, including exhaustive 16-bit inputs
for the basic member reduction on all thirty objects and read-count checks for
ordinary/volatile pointers and mutations between sums. Their expected cycle
values are checked against all 65,536 inputs to the original linked mixer block.
The thirty global-member list-sweep objects pass **1,920 calls**, comparing all
packet and voice bytes and callback counts, including 64-node walks.

Canaries **1739–1740** preserve a separate frame-growth follow-up discovered by
an isolated register-index assignment chain. Nineteen of thirty configurations
compile: all fifteen O0 cases and four older O4 builds. Eighteen execute
correctly for indices 0/1/31/63. GC/1.1p1 O4 emits a bad epilogue that loses the
LR reload after expanding the saved-register range; the other eleven O4 builds
decline because the frame lacks capacity for three saved registers. These are
known failures, excluded from the successful 180-object execution set above.

Validation:

- Captured paired-memory matrix: **300/300 whole-object exact**.
- Index panel: **1,096 objects unchanged**, retaining **972 known reference
  matches**, and **578 identical declines**; no timeouts.
- Cumulative metadata panel against `db48e092`: **1,494 objects unchanged**,
  retaining **1,179 known reference matches**, and **704 identical declines**;
  no timeouts.
- **16 anchor tests pass**. **408 parser tests pass**, excluding the two
  previously documented parser failures. The new parser test checks C/C++
  local-pointer volatility and shadow-name facts.

Artifacts and reproducible local probes are in `target/check_member_lookup*.py`,
`target/probe_member_lookup_full_ax.py`, `target/check_full_ax_sync_execution.py`,
`target/member-lookup-canaries/`, and `target/member-lookup-full-ax/`.
The complete object SHA-256 is
`1a8fed48a1634517cd66e23f09754e75274d170ca826e61408ff96650605e7e3`.
Fresh reference-compiler objects remain blocked by the six existing wibo
processes, still in kernel U state after more than three hours. No new wibo
processes were launched and no fresh full-project panel improvement is claimed.

## Retained inline cursor calls and AX initialization, 2026-09-07

BfBB's `__AXDumpVPB` now expands in the mutable `pvpb` list walk inside
`__AXSyncPBs`. The expanded caller also gets past its array-address cache calls
and initial cycle arithmetic. The configured file now reports its next actual
failure: the three-table cycle sum indexed by the `pvpb->pb.mixerCtrl` member.
The existing sum selector accepts register-derived indices; member-derived
indices and their load-sharing schedule remain unimplemented. This advances
full-file lowering but does not yet produce a complete AX object.

The retained-inline path now uses its existing hygienic parameter temporaries
for scalar arguments read from changing caller variables. It captures the
argument once at each call site, including inside a loop, instead of rejecting
it because the variable is reassigned elsewhere. Ordinary automatic-inline
eligibility remains governed by its existing policy. The escape check remains
in force. A new unit regression checks that both expanded calls use the same
captured cursor and that the caller's subsequent cursor assignment is separate;
the previous changing-value test now checks capture while retaining its escape
rejection assertion.

Two shared lowering fixes handle the next real-source failures. Call scheduling
now distinguishes array-address arguments from scalar global loads before
applying the constant-after-load restriction. Wide integer constant addition
can now produce a scratch result through a separate running register: the high
adjustment uses that register, and the final `addi` writes r0 without interpreting
r0 as a literal-zero base. This handles AX's `(get_cycles() + 0x10000) - 0x55f0 +
extra`, including unsigned wraparound. When required inline expansion succeeds
but subsequent lowering fails, the driver returns that underlying diagnostic
instead of incorrectly blaming the original skipped inline call.

Canaries **1717–1726 improve 15/150 → 150/150 candidate compilations** against
frozen baseline `8fb69501`, covering all fifteen builds at O0/O4. The fifteen
already-supported O0 array-call objects remain byte-identical. All **9,600
candidate execution calls pass**, checking mutable list cursors, scalar and
global argument snapshots, callback order, global mutation between argument
uses, array addresses and sizes, arithmetic overflow, and the AX dump transaction.
Every callback clobbers caller-saved registers; stack restoration and
callee-saved registers are checked. All 150 final-compiler objects are
byte-identical to the executed objects.

The dump reduction preserves the original 556-byte voice and 244-byte packet
layouts, conditionally calls depop, clears the chained state/update fields, and
calls the callback-stack helper. Across thirty candidate objects, its walks
match **3,990 original inlined dump-block executions** from BfBB's linked
`__AXSyncPBs` at `[0x801BA274, 0x801BA2BC)`. The comparison covers all packet and
voice bytes and callback arguments. The reduction's additional caller-side index
clear is applied after each original block before comparison. The initial-cycle
reductions also match **1,920 original linked arithmetic-block executions** at
`[0x801BA1A4, 0x801BA1B4)`, seeding the call result and extra-cycle input. These
are execution comparisons of reductions, not complete `__AXSyncPBs` parity.
The original code comes from the previously fingerprinted DOL extraction in
`target/bfbb-ax-linked/AXSyncPBs.bin`.

Validation:

- Captured paired-memory matrix: **300/300 whole-object exact**.
- Index panel: **1,096 compiled objects unchanged**, retaining **972 known
  reference matches**, and **578 identical declines**, with no timeouts.
- Cumulative metadata panel against `db48e092`: **1,494 compiled objects
  unchanged**, retaining **1,179 known reference matches**, and **704 identical
  declines**, with no timeouts.
- **117 inline-expansion tests pass**, excluding the previously documented
  `composes_zero_argument_embedded_asm_at_a_nested_call_site` failure.
- The complete isolated BfBB service object is byte-identical to the previous
  milestone's object, retaining its 2,160 linked-function execution comparisons.

Scripts and measurements are in `target/check_inline_chain.py`,
`target/check_inline_chain_execution.py`, `target/check_inline_chain_regression.py`,
`target/check_inline_chain_metadata.py`, `target/probe_inline_chain_full_ax.py`,
`target/inline-chain-canaries/`, and `target/inline-chain-full-ax/`. Fresh
reference-compiler objects remain unavailable because the previously identified
wibo processes are still stuck; no new compiler-object matches or full-project
panel improvement are claimed beyond the captured checks above.

## Complete BfBB AX service execution, 2026-09-07

The configured GC/1.2.5n BfBB AX file now lowers all of `__AXServiceVPB`.
A source prefix ending before `__AXSyncPBs`, compiled with the project's exact
flags and include paths, improves from a baseline `fd0aaa00` decline to a
2,840-byte service function. Against the original game's 1,868-byte linked
function, all **2,160 execution comparisons pass**. This is complete service
function execution evidence, not instruction/object parity or a completed AX
translation unit. The full configured file now stops in `__AXSyncPBs` at the
skipped inline call `__AXDumpVPB`. Reference project files were not changed.

Three shared lowering paths now accept the forms encountered in that service:

- A discarded register-resident local read emits no instruction. AX explicitly
  contains `dst; // fixes reg alloc`; it is not a residual assignment. Bare
  frame-backed values and globals remain outside this new no-op case.
- Taking the address of an embedded array (`&pvpb->updateData`) reuses the
  existing member-address storage/offset logic for `MemberAddress` nodes,
  including automatic aggregates and globals.
- A cast of an address-valued member expression materializes its address in a
  virtual register. The cast's pointee supplies the load/store width, preserving
  word copies through nested halfword-member addresses.

Execution testing also found that the parser unconditionally unwrapped a
member base dereference: `(*p)->data` incorrectly became `p + offset`. Only the
`(*p).data` spelling now unwraps that dereference. The arrow form retains its
pointer load; a parser regression checks both forms, and candidate execution
checks the loaded address on every build at both optimization levels.

Canaries **1709–1716 improve 30/120 → 120/120 compilations** across fifteen
builds at O0/O4. They cover discarded scalar/pointer parameters, the original
52-word COPYALL sequence, pointer/global/frame/nested embedded-array addresses,
and cast member loads and stores. All **20,160 candidate calls pass**, including
forward/backward overlapping copies, aliasing cast addresses, integer wraparound,
callee-saved registers, stack restoration, and a frame-address callee that
clobbers caller-saved registers. The thirty already-supported COPYALL objects
remain byte-identical to the baseline. The 52-copy count deliberately follows
the project source despite its comment claiming a 0xF4-byte copy.

The complete-service comparison uses every individual sync bit, zero, all-bit
combinations, and 100 deterministic random flags; indices 0, 1, 31, 63; and
update counts 0, 1, 7, 64. Each case compares the full voice object, all DSP
parameter blocks, all update blocks, the ITD buffer, and the voice counter after
running both original and candidate code. Stack restoration and nonvolatile
registers are checked too. Thus **22,320 candidate calls pass in total**. The
original function is extracted from BfBB's fingerprinted GQPE78 DOL with
`tools/extract_dol_reference.py`; its manifest and the candidate object are in
`target/discarded-value-full-ax/`.

Validation after the parser correction:

- Captured paired-memory matrix: **300/300 whole-object exact**.
- Index regression: **1,096 compiled objects unchanged**, retaining **972 known
  reference matches**, plus **578 identical declines**; no timeouts.
- Cumulative metadata regression against `db48e092`: **1,494 compiled objects
  unchanged**, retaining **1,179 known reference matches**, plus **704 identical
  declines**; no timeouts.
- Bare volatile-global, volatile-local, and address-taken-parameter reads:
  **90/90 unchanged declines**. The new no-op case does not silently accept them.
- Parser tests: **407 pass**, with the same two previously documented failures
  (`recovers_friend_bearing_layouts_and_expression_template_arguments` and
  `retains_brace_initialized_aggregate_image_from_discarded_inline`). Filtering
  those two produces a passing suite, including the new arrow/dot regression.

Reproduction scripts and results: `target/check_discarded_values.py`,
`target/check_discarded_execution.py`,
`target/check_discarded_service_execution.py`,
`target/probe_discarded_value_service.py`,
`target/probe_discarded_value_full_ax.py`,
`target/check_discarded_value_regression.py`, and
`target/check_discarded_value_metadata.py`. Fresh reference compiler objects
remain unavailable while the same six wibo processes remain stuck in kernel
uninterruptible state. No new compiler-object matches or full-project panel
improvement are claimed beyond the captured checks above.

## Complete AX lookup sums and retained table bases, 2026-09-07

The complete three-lookup AX cycle expression and its addition to an existing
cycle count now compile for all fifteen builds at O0/O4. Against frozen baseline
`4b5eaede`, new canaries 1701–1708 improve **0/120 → 120/120 candidate
compilations**. These include sixty project-derived sum/accumulator objects,
thirty mixed sum objects, and thirty functions that retain table bases across a
call. Fresh compiler-reference objects remain unavailable, so no new whole-object
matches are claimed. Existing canary 1683 also compiles in all thirty configurations;
its code, data, and relocations equal the executed complete-sum reduction, apart
from the separate file-symbol identity.

`global_lookup_sum` plans additive chains of masked integer global-array loads,
integer constants, and resident word values. Validation finishes before emission.
O4 composes the tail first, shares each table's base, prepares the leading address
before the pending tail addition, and keeps loaded values in separate virtual
registers. It consumes an existing section anchor when one is already planned.
O0 evaluates complete lookup operands in source order while reserving all input
registers. The common lookup recognizer and fused-index selector are reused;
ordinary two-load expressions retain their measured selectors. Memory-loaded
indices and their common-subexpression handling remain outside this sum planner.

Canaries 1701–1704 preserve BfBB's original tables, complete lookup right-hand
side, and accumulated-cycle form with the mixerCtrl field extracted as a u16
parameter. All **3,932,160 candidate calls across sixty objects** pass exhaustive
comparison for the 65,536 possible mixer values against the original linked
`__AXSyncPBs` observations, including accumulator overflow. The final compiler's
sixty objects are byte-identical to the executed objects. This is an execution
comparison of reductions, not full-function instruction or object parity.

Canaries 1705–1706 add five-term sums, large and negative constants, repeated
scalar inputs, mixed byte/signed-halfword/word loads, volatile reads, and a global
store through r0. Their thirty objects pass **7,800 calls**. Canaries 1707–1708
load a value before a call, pass all three tables to a mutating callee, then sum
new table values and the saved value. All thirty objects pass **780 calls** with
caller-saved register clobbers, array-read counts, stack restoration, and
callee-saved register preservation checked. Eight configurations use a retained
section anchor. All **3,940,740 candidate calls pass**.

The across-call case exposed a prologue scheduler bug: the anchor-only prefix
matcher also matched a frame with a local initializer before the saved parameter
copy. Retargeting its anchor high half to r3 destroyed the incoming parameter.
The scheduler now preserves the allocated staging register when r3 can still be
read before a definition. Both relevant scheduler tests pass, including the new
initializer/branch regression.

The captured memory-operand matrix retains **300/300 exact objects**. The masked
index panel retains **1,341/1,470 exact objects** and all candidate compilations.
Direct index regressions retain **1,096 compiled objects**, **972 previously
reference-exact objects**, and **578 decline diagnostics** (1,674 runnable pairs
from 2,115 slots, 441 prior exclusions). A cumulative metadata comparison against
the preserved `db48e092` compiler also retains **1,494 compiled objects**, **1,179
previously reference-exact objects**, and **704 decline diagnostics** (2,198
runnable pairs from 2,940 slots, 742 prior exclusions). Neither panel times out.
The native compiler and oracle build successfully.

The configured full BfBB AX source was retried with the inventory's GC/1.2.5n
flags, configuration `28bbd3861e4ba5aaaa7937e409f8af691d305fd46ad02945195bc980beb94522`.
Both frozen and current compilers stop earlier in `__AXServiceVPB`: a discarded
`Variable("dst@4")` expression reaches the “only a call may be a bare statement”
guard. Full AX-file compilation, member-derived index CSE, and fresh full-project
object parity remain unfinished. Evidence is under `target/lookup-sum-*` and
`target/check_lookup_sum_*`; captured matrices use the `lookup-sum` label.

## Shifted AX lookup composition and linked-DOL references, 2026-09-07

The first two AX mixer-cycle lookups now compile together for all fifteen builds
at O0/O4. Against frozen baseline `acca5a7f`, new canaries 1697–1700 improve
**0/60 → 60/60 candidate compilations**. Thirty are the project-derived lookup
pair, and thirty exercise other shifted inputs. Their fresh compiler-reference
objects are unavailable, so this checkpoint claims no new whole-object matches.
The captured memory-operand panel retains **300/300 exact objects**, and the
masked-index panel retains **1,341/1,470 exact objects** with all 1,470 compiling.

A provenance check found that Strikers' AX source is absent from its active
build inventory, configured source list, and linked symbols. Canaries 1683 and
1695–1696 now label it as source-only. BfBB actively configures the equivalent
expression in `src/dolphin/src/ax/AXVPB.c:697-699` for GC/1.2.5n. Its original
`orig/GQPE78/sys/main.dol` contains `__AXSyncPBs`, and both cycle tables match the
source initializers byte-for-byte. This provides original linked-code evidence
while the separate reference-runner processes remain stalled.

New `tools/extract_dol_reference.py` extracts a named function using the original
DOL and project's sized symbol map. It writes the bytes and a manifest pinning
the DOL, symbol map, address range, and extracted-code hashes. Its reference kind
is explicitly `linked_dol_function`; it does not manufacture a relocatable
compiler-reference object. Four tests cover address mapping, invalid/cross-section
ranges, truncated data, overlapping sections, and ambiguous or unsized symbols.
For example:

```sh
python3 tools/extract_dol_reference.py \
  --dol ../Metrowerks/reference_projects/battle_for_bikini_bottom/orig/GQPE78/sys/main.dol \
  --symbols ../Metrowerks/reference_projects/battle_for_bikini_bottom/config/GQPE78/symbols.txt \
  --symbol __AXSyncPBs --output target/bfbb-ax-linked/AXSyncPBs.bin
```

The extracted function is **632 bytes at 0x801BA15C**. Its mixer-cycle block
`[0x801BA224, 0x801BA260)` fuses each right shift, mask, and word scale into one
rotate-and-mask. The shared masked-index representation now records that fused
rotation separately from the element scale. The existing two-global-load
selector opts into shifted inputs and unsigned narrow inputs when every selected
bit lies inside the input width. Existing plain and biased-pointer callers keep
their recognition. No source-symbol-specific emitter was added.

O0 testing exposed an existing legacy-address bug: an integer lookup returning
through r0 also used r0 as a displacement base, which denotes address zero on
PowerPC. The legacy global-array emitter now gives that address a separate
virtual register, including the retained-section-base path.

Unicorn validates the original linked mixer block for all **65,536 u16 inputs**,
including overflowing cycle accumulators, and records the first two table values
before the full sum combines them. All **1,966,080 candidate calls across 30 AX
pair objects** agree with those linked-code observations. The other 30 objects
add **6,240 calls**, covering signed inputs, shifts 29/31, unsigned byte/halfword
inputs, mixed element widths, and memory-read counts. All **1,972,320 candidate
calls pass**. This is an exhaustive input-domain execution comparison for the
reduced pair, not an instruction or whole-object parity claim for that reduction
or the full AX function.

Direct frozen/candidate index regressions retain **1,096 compiled objects**,
including **972 previously reference-exact objects**, and all **578 decline
diagnostics** (1,674 runnable pairs from 2,115 slots, 441 prior exclusions), with
no timeouts. The native compiler and oracle build successfully. The complete
three-lookup sum still reaches the additive-chain allocator guard on GC/1.2.5n;
full AX-file and full real-project parity remain unmeasured at this fingerprint.
Evidence is under `target/shifted-lookup-*`, `target/check_shifted_lookup_*`, and
`target/bfbb-ax-linked`; captured matrices use the `shifted-lookup` label.

## Biased pointer indices and AX lookup isolation, 2026-09-07

Against frozen baseline `5e5241e4`, the captured memory-operand matrix improves
**270/300 → 300/300 whole-object exact**, with all 300 candidate objects compiling
and no lost matches. This completes the ten-shape focused matrix, not the full
corpus or real-project objective. The final **30/30 biased-index pairs** cover
`table[index & 3] op table[(index + 1) & 3]`, six integer operations in both source
orders, fifteen builds, and O0/O4. References are the original captured objects.

The shared mask representation now has an optional signed immediate bias, selected
explicitly by pointer callers. Global address selection retains its existing
recognition. Ordinary biased pointer loads prepare the input before masking, and
the paired selector tracks the prepared input separately from the scaled offsets
and loaded values. The oldest four builds complete a biased primary's offset
before the plain secondary, then load the primary first. Later builds prepare the
bias, scale the plain index, scale the biased index, and load the plain operand
first. A profile property owns that distinction. O0 evaluates each full operand
independently and also supports two biased operands. Loaded biased indices and
optimized pairs with two biased indices still need their own issue policies.

Canaries 1691–1692 preserve the captured O4/O0 body. Their **30 compiled objects**
match its code, symbols, and comment metadata apart from renamed file symbols.
Canaries 1693–1694 add independent inputs, negative bias, commuted addition,
signed halfwords, byte loads, discontiguous masks, and volatile accesses. Their
**30 candidate objects** compile and execute; fresh reference comparisons are
pending and are not counted as exact. Unicorn validates **38,880 paired
reference/candidate calls**, including unsigned wraparound at `0xffffffff`, and
**7,920 candidate-only calls**, checking memory-read counts and pointer/input
lifetimes. All **46,800 calls pass**.

The masked-index panel retains **1,341/1,470 exact objects** and all candidate
compilations. Frozen/candidate index regressions preserve **1,096 compiled
objects**, including **972 previously reference-exact objects**, and **578 decline
diagnostics** (1,674 runnable pairs from 2,115 slots, 441 prior exclusions), without
timeouts. All **45 version tests pass**; the native compiler and oracle build.

Isolating the Strikers AX expression in canary 1683 changes the next action:
its original main lookup and shifted auxiliary lookup already compile separately
on GC/2.6 with the original u16 input and table initializers. New project-derived
canaries 1695–1696 preserve those single lookups. Their objects are byte-identical
to the frozen baseline, and **131,072 exhaustive candidate calls** validate both
lookups for all 65,536 u16 inputs. This verifies existing standalone semantics,
not a new matching-output gain. Fresh reference comparisons remain pending.
The original two-lookup combination still reaches a non-leaf operand diagnostic;
the full expression still reaches the additive-chain allocator guard. Composition
of these lookup results is the immediate project-derived gap, rather than basic
execution of the shifted lookup alone.

The full real-project panel remains unmeasured at this fingerprint because the
existing reference-runner processes are still stalled. Evidence is under
`target/biased-index-*` and `target/check_biased_index_*`; matrices use the
`biased-index` label in `target/memory-operands-probes` and
`target/masked-index-probes`.

## Two distinct global-array loads, 2026-09-07

Against frozen baseline `a3b47752`, the captured memory-operand matrix improves
**240/300 → 270/300 whole-object exact**, with compilation improving by the same
amount and no lost matches. All **30/30 two-table pairs** now match: six integer
operations in both source orders, fifteen compiler builds, and O0/O4. The remaining
30 configurations still decline on the more complex computed pointer index.
All references come from the original capture before the runner stalled.

The new `two_global_loads` selector exposes both independent address chains to
one versioned placement policy. GC/1.1 and GC/1.2.5[n] finish an explicit first
address before starting the second; GC/1.1p1 starts both high halves early. Middle
builds complete the secondary indexed base first, while GC/3 and Wii retain both
high-half registers as completed bases. O0 reuses ordinary operand evaluation
with the second operand's inputs reserved. Mask selection, relocation recording,
and integer load opcodes remain shared with the existing builders. The selector
accepts distinct full-size integer arrays and register-derived masked indices;
member-derived indices, shared bases, and owned static-data anchors retain their
existing paths.

Canaries 1687–1688 cover the captured O4/O0 bodies. Their **30 compiled objects**
match the captured code, symbols, and comment metadata apart from renamed file
symbols. Canaries 1689–1690 add independent parameter indices, signed halfwords,
mixed byte/halfword widths, discontiguous masks, and volatile reads. All **30
candidate objects** compile and execute; their fresh reference comparisons remain
pending and are not counted as exact objects. Unicorn validates **56,160 paired
reference/candidate calls**, including randomized words, overflow, all operand
orders, relocated addresses with high-half carry, and overlapping arrays. The
extra candidate probes add **4,680 calls**, checking two reads per expression
and preserving the second index across address construction. All **60,840 calls
pass**.

The masked-index panel preserves **1,341/1,470 exact objects** and all candidate
compilations, including the Wind Waker getter reduction. Direct frozen/candidate
index regressions preserve all **1,096 compiled objects**, including **972
previously reference-exact objects**, and all **578 decline diagnostics** (1,674
runnable pairs from 2,115 slots, 441 prior exclusions), without timeouts. All **45
version tests pass**, and the native compiler and oracle build successfully.

The Strikers AX reduction in canary 1683 was retried on GC/2.6 and still reaches
the additive-chain allocator diagnostic. Its complete expression also needs
shifted indices and static-table address handling; this checkpoint establishes
the two-table component, not full AX compilation. The full real-project panel
remains unmeasured at this fingerprint because the existing reference-runner
processes are still stalled. Evidence is under `target/two-global-*` and
`target/check_two_global_*`; captured matrices use the `two-global` label in
`target/memory-operands-probes` and `target/masked-index-probes`.

## Shared global-array load pairs, 2026-09-07

Against frozen baseline `c1355bc6`, the captured memory-operand matrix improves
**210/300 → 240/300 whole-object exact**, with compilation improving by the same
amount and no lost matches. All **30/30 shared-array pairs** match: a masked word
subscript combined with element zero, six integer operations in both source
orders, fifteen compiler builds, and O0/O4. The remaining 60 configurations
still decline: two global-array subscripts and a more complex computed pointer
index. The references are the original captured objects, not fresh compiler runs.

A dedicated version profile describes explicit element addresses on the oldest
four builds, updating constant loads on the middle eight, and separate low-half
relocations on the newest three. O0 uses the existing masked-address builder and
evaluates complete operands separately. Wii's O0 reverse subtraction keeps the
indexed primary in the completed address register; the address builder now accepts
that placement without changing ordinary subscript allocation. The earlier
repeated-global-pointer guard now distinguishes declared arrays from pointer
variables, allowing array pairs to reach their selector. This path currently
covers full-size word arrays and register-derived masked indices; member-derived
indices and owned static-data anchors still need characterization.

Canary 1682 now covers the captured O4 body, and 1684 adds O0. All **30 canary
objects** match the captured bodies' code, symbols, and comment metadata apart
from renamed file symbols. Canaries 1685–1686 add volatile reads and mask 85;
all **30 candidate objects** compile and execute, but their fresh reference
comparisons are pending and are not counted as exact. Unicorn validates
**18,720 reference/candidate calls** and **9,360 candidate-only calls**, including
randomized words, overflow, both operand orders, reads that select the same
element, high-half relocation carry, and negative low-half displacements. Every
call performs exactly two memory reads; all **28,080 calls pass**.

The masked-index panel retains **1,341/1,470 exact objects** and all candidate
compilations, including the Wind Waker getter reduction. The small-array panel
retains **180/180 exact objects**. Direct frozen/candidate regression comparisons
preserve **1,096 compiled objects**, including **972 previously reference-exact
objects**, and all **578 decline diagnostics** (1,674 runnable pairs from 2,115
slots, 441 prior exclusions), with no timeouts. All **45 version tests pass**.
The native compiler and oracle build successfully. The full real-project panel
remains unmeasured at this fingerprint: the existing reference-runner processes
are still stalled. The Strikers AX reduction in canary 1683 remains the next
project-derived frontier involving two tables and an additive chain.

Evidence is under `target/shared-global-*` and `target/check_shared_global_*`;
matrices use the `shared-global` label in `target/memory-operands-probes`,
`target/masked-index-probes`, and `target/masked-small-probes`.

## Masked global-array/scalar load pairs, 2026-09-07

Against frozen baseline `db48e092`, the captured memory-operand matrix improves
**180/300 → 210/300 whole-object exact**, with candidate compilation improving
by the same amount and no lost matches. All **30/30 global-array/scalar pairs**
match, covering six integer operations in both source orders, fifteen builds,
and O0/O4. The remaining 90 configurations still decline: a constant subscript
sharing the global-array base, two global-array subscripts, and a more complex
computed pointer index. All 300 references remain available from the capture
made before the reference runner stalled.

`masked_global_address` now owns address construction separately from the
final load. Ordinary subscripts use that same builder with their existing
register and relocation rules. A global load pair can issue its scalar read
after the high-half address instruction on the oldest builds, or after the
complete indexed address on later builds. Reverse subtraction reserves r0 for
the scalar, so the address builder keeps its offset and low-half value in
separate registers. O0 retains complete primary-first operand evaluation.
The new pair path covers integer full-size global arrays with SDA scalar
operands; small-array pairs, absolute scalar addressing, and two global-array
reads still need additional schedules.

Instruction bytes matched first; O0 symbol registration required a separate
fix. The oldest four and newest three builds register the array address before
the function symbol, but create the scalar value reference afterward. Middle
builds retain the full reference-discovery stream before the function. Explicit
`body_value_references` metadata now distinguishes the scalar read from early
address discovery. The object writer applies its existing function-order
profile to those phases without changing relocation order. Its expanded phase
test covers all four relevant profiles with and without a later value reference;
all **40 object-writer tests pass**.

Existing canary 1674 now covers the supported O4 body, and new 1679 covers O0.
Their **30 compiled objects** match the captured reference bodies' executable
bytes, symbols, and comment metadata apart from changed file-symbol names.
New canaries 1680–1681 add mixed widths, member-derived indices, discontiguous
masks, and volatile globals. All **30 candidate objects compile and execute
correctly**; their fresh reference comparisons remain pending and are not
counted as exact objects. Canary 1682 preserves the shared-global-base failure
already represented by captured objects. Canary 1683 reduces Super Mario
Strikers `src/Dolphin/ax/AXVPB.c` using the original two mixer-cycle tables and
the cycle-increment right-hand side from `__AXSyncPBs`, with its u16 field
extracted as a parameter. It currently reaches the additive-chain allocator
diagnostic on GC/2.6. This is a project-derived frontier case, not a claim of
full AX-file compilation or a freshly measured reference match.

Unicorn validates **56,160 reference/candidate calls across 30 paired objects**
with randomized values, all operand orders, overflow, and relocated scalar/
array storage that can overlap. The mixed-width/member cases add **6,240
candidate calls across 30 objects**, checking results and the number of memory
reads. All **62,400 calls** pass. The shared address refactor preserves
**1,341/1,470 exact masked-index objects**, including the Wind Waker getter
reduction, all 1,470 candidate compilations, and **180/180 small-array matches**.
No match is lost in either matrix.

Direct frozen/candidate regression comparisons preserve all **1,096 compiled
index objects**, including **972 previously reference-exact objects**, and all
578 decline diagnostics (1,674 previously runnable pairs from 2,115 slots,
441 prior exclusions). The metadata panel preserves all **1,494 compiled
objects**, including **1,179 previously reference-exact objects**, and all 704
decline diagnostics (2,198 previously runnable pairs from 2,940 slots, 742
prior exclusions). Neither panel times out. The full real-project panel remains
unmeasured at this fingerprint because fresh reference-runner processes are
still stalled. Evidence is under `target/global-load-pair-*`; matrix artifacts
use the `global-pair` label in `target/memory-operands-probes`,
`target/masked-index-probes`, and `target/masked-small-probes`.

## Two computed pointer loads and array shadowing, 2026-09-07

Against frozen baseline `e985e342`, the captured 300-pair memory-operand matrix
improves **150/300 → 180/300 whole-object exact**, with compilation improving
by the same amount. All **30/30 two-pointer pairs** now match, covering six
integer operations in both source orders, fifteen builds, and O0/O4. Every
previous exact match is retained. The remaining 120 configurations still
decline: three global-operand shapes and a more complex computed index. All
300 reference objects were captured before the ongoing reference-runner stall;
there are no missing reference objects in this matrix.

The existing `indexed_load_pair` owner now shares resident-pointer/mask
recognition between one- and two-subscript paths. At O4 it computes the
secondary offset through r0, then the primary offset in a separate virtual
register, before loading either operand. The oldest four builds issue the
secondary load first; later builds issue the primary first. A dedicated version
profile property records that distinction. O0 evaluates one complete operand
at a time and keeps the primary result outside the input registers. This path
currently requires register-derived indices; two member-derived index loads
need their own dependency schedule.

O0 comparison exposed an independent binding bug: the pointer parameter
`other` was treated as the file-scope array of the same name, creating a global
relocation and reading the wrong storage. Shared global-array address-extent
lookup now honors resident and frame-local bindings before classifying a name
as a global array. The captured two-pointer source exercises that shadowing,
so matching the entire object also verifies removal of the spurious relocation.

Existing canary 1675 now covers the supported O4 body; new canary 1676 preserves
its O0 counterpart. All **30 canary objects** match the captured reference
bodies' executable bytes, symbols, and comment metadata apart from their changed
file-symbol names. Two further canaries (1677–1678) exercise independent indices,
unsigned bytes, signed halfwords, discontiguous masks, volatile reads, and
parameters shadowing arrays with different element types. Their **30 candidate
objects compile and execute correctly**, but fresh reference comparisons remain
pending; they are not counted as exact objects.

Unicorn validates **37,440 reference/candidate calls across 30 paired objects**
with randomized values, both operand orders, aliased pointers, and overflow.
The independent-index/type canaries add **7,200 candidate calls across 30
objects**, checking results, two memory reads per expression, and absence of
relocations to the shadowed globals. All **44,640 calls** pass. The 45 version
profile tests pass. The full prior masked-index matrix retains **1,341/1,470
exact objects**, including the Wind Waker getter reduction, and all 1,470
candidate compilations. Neither matrix loses a match.

The direct frozen/candidate index regression comparison again finds all
**1,096 compiled objects byte-identical**, preserving all **972 previously
reference-exact objects**, and all **578 decline diagnostics unchanged**.
These are 1,674 previously runnable pairs from 2,115 slots, with 441 prior
exclusions and no timeouts. The full real-project panel has not been remeasured
while fresh reference-runner processes remain stalled; this milestone makes no
new full-file or project-build parity claim. Evidence is under
`target/dual-load-*`; matrix artifacts use the `dual` label in
`target/memory-operands-probes` and `target/masked-index-probes`.

## Masked pointer load pairs, 2026-09-07

Against frozen baseline `8bccd8e1`, a 300-pair matrix improves **0/300 →
150/300 whole-object exact**, and candidate compilation improves **30/300 →
150/300**. All 300 reference objects were captured successfully before the
reference runner stalled. The matrix covers ten operand shapes, six integer
operators in both source orders, all fifteen builds, and O0/O4. The supported
five shapes pair a masked pointer subscript with a constant-index or member
load, including unsigned byte, halfword, word, and member-derived indices. All **150/150
supported pairs** are exact. Global operands and two computed subscripts still
decline in the other 150 configurations.

`expressions/indexed_load_pair.rs` owns operand placement and issue order;
mask selection remains shared with ordinary subscripts, and the existing
arithmetic emitter owns the final operation. O4 starts the index calculation,
issues the independent load in its first latency slot, then completes the
indexed load. Reverse subtraction gives the indexed result its own virtual
register, with separate lifetimes for the loaded and scaled index. O0 retains
primary-first evaluation and avoids the input registers for the primary result.
An explicit version-profile property preserves the consumed scaled parameter
register on the oldest four builds; byte indices and member-derived indices
follow their measured allocation rules. Selection excludes nonresident bases,
large displacements, floating types, signed bytes requiring extension, and
complex member address calculations.

Ten permanent canaries (1664–1673) preserve the supported matrix bodies. Their
150 compiled objects match the captured references' executable bytes, symbols,
and comment metadata, allowing for the intentionally changed file-symbol names;
this is distinct from the unmodified matrix's whole-object comparison. Two
additional canaries (1674–1675) preserve global-scalar and two-computed-pointer
failures for subsequent work. The previous masked-index edge panel improves
**60/180 → 105/180 exact**, with compilation **150/180 → 180/180**, no lost
matches, and no unknowns. Its original two-table-read failure now matches on
all 30 build/optimization combinations; reused-member O0 adds 15 more matches.

Unicorn executes **187,200 reference/candidate calls across 150 paired objects**
with randomized values, all six operations, both operand orders, aliased inputs,
and 32-bit overflow. Another **450 candidate calls across 30 objects** validate
discontiguous masks, negative displacements, separate pointer operands, and
exactly two volatile reads even when both addresses coincide. The edge panel
adds **1,440 paired execution calls across 180 objects**. All **189,090 calls**
pass. The 45 version-profile tests also pass, and 30 signed-byte probes confirm
that unsupported extension still declines. A larger additive chain such as
`(table[index & 3] + table[0]) + index` still reaches the existing additive-chain
allocator diagnostic; this change does not establish arbitrary expression-tree
lowering.

The existing index regression selection uses direct frozen/candidate compiler
comparisons while the reference runner is unavailable: all **1,096 compiled
objects are byte-identical**, retaining all **972 previously reference-exact
objects**. All **578 decline diagnostics** are unchanged. These are the same
1,674 previously runnable pairs from 2,115 slots, with 441 prior exclusions;
neither compiler times out. The real-project panel was **not rerun** at this
fingerprint. Its latest measurement remains the preceding checkpoint's 132
configurations, including full-file Wind Waker and Mario Kart timeouts; no new
full-file or project-build parity is claimed here. Fresh reference launches
hung before compiler execution, so this checkpoint uses the already captured
matrix objects rather than treating those hangs as compiler failures. Evidence
is under `target/memory-operands-*`, with edge results labeled `memory-operands`
in `target/masked-index-edge-probes`.

## O0 and small masked-array accesses, 2026-09-07

Against frozen baseline `b23f645b`, the 1,470-pair masked-index matrix improves
**657/1,470 → 1,341/1,470 whole-object exact pairs**, preserving every prior
match and all candidate compilations. The existing O0 global-array canary 1659
improves **0/15 → 15/15**. Four new cases (1660–1663) cover O0 pointer bases,
Wind Waker's getter at O0, and small global arrays at O4/O0. They improve
**0/60 → 57/60**: the three synthetic cases each match 15/15, and the original
Wind Waker getter body matches 12/15 at O0. The combined 1656–1663 selection
improves **42/120 → 114/120**, with all references runnable and candidates
compilable. The newer getter objects retain static-local identity differences.

The selector retains separate mask and scale instructions at O0. Pointer
bases use indexed loads. Full global arrays use the existing versioned address
convention: older builds keep the scaled offset while constructing an explicit
element address, and newer builds retain the separate base and index. Full
O0 array-address discovery also precedes the function symbol; this uses the
existing function metadata instead of changing the writer's general ordering.
SDA references retain their ordinary symbol event order.

Small arrays use SDA21 and indexed loads on every build. Their optimized
parameter/member schedules differ, while O0 completes the mask and scale before
the base. A separate 180-pair byte/halfword/word panel improves **0/180 → 180/180
exact objects**, and compilation improves **120/180 → 180/180**; masked small
byte-array loads no longer decline. The 180-pair edge panel improves
**30/180 → 60/180**, retaining 150 compilations. Its broader two-table-read
expression still declines on the same 30 configurations. A supplementary
mixed-expression probe confirms exact O0 global-array reads with a reused
parameter on GC/1.1, GC/2.6, and GC/3.0a3; combining two memory reads remains
outside the current operand lowering.

Unicorn validates **1,080 paired main-matrix objects with 25,920 calls**, now
covering both O0 and O4. Small-array execution adds **1,440 calls across 180
pairs**, and repeated-index/commuted/high-bit/zero-mask edges add **1,200 calls
across 150 pairs**. All **28,560 calls** pass. Eight focused driver tests pass.
The index regression panel retains **972 exact objects across 1,674 runnable
pairs** (2,115 slots, 441 exclusions), and the metadata panel retains **1,179
across 2,198 runnable pairs** (2,940 slots, 742 exclusions). No exact matches
are lost and neither regression panel times out.

Every individual object and code verdict remains unchanged in the
**132-config real-project panel: 47 BYTE, 16 DIFF, 59 compiler DEFER, six
HARNESS, four missing dependencies**. Code remains **46/109 exact measured
objects**, five empty and eighteen unmeasured; all 51 measured partial
translation units remain nonexact. The complete Wind Waker and Mario Kart
files still hit the 30-second cap, so the reductions do not establish full-file
or project-build parity. Evidence is under `target/masked-o0-*` and
`target/masked-small-*`; matrix results use the `masked-o0` label in
`target/masked-index-probes` and `target/masked-index-edge-probes`.

## Masked array-index selection and scheduling, 2026-09-07

Against frozen baseline `ca796627`, Wind Waker's original module-type getter
body in canary 1656 improves **0/15 → 12/15 whole-object exact pairs**. The
remaining newer-build objects have static-local identity differences; their
optimized instruction bytes now match. Two new optimized canaries (1657–1658)
cover global arrays and pointer bases and each improve **0/15 → 15/15**. The
O0 companion 1659 remains nonexact. Combined, the four canaries improve
**0/60 → 42/60**, with all references runnable and all candidates compilable.

A separate expression selector combines an integer index mask with the byte,
halfword, or word element scale. Contiguous results use `rlwinm`; discontiguous
immediate masks retain AND followed by the scale. Parameter and loaded-member
indices retain their measured address-setup order, and the existing profile
separates legacy explicit element addresses from indexed loads. A new profile
setting captures newer builds retaining the high-half address register.
Pointer bases share the mask/scale selector without global-address setup.
The O0 path retains separate lowering. Small global arrays, wider elements,
and other source expressions continue through their existing owners.

The scheduler now treats record-form immediate ANDs as boundaries, so later
address materialization cannot move ahead of the completed mask. A focused
scheduler test checks both immediate and shifted-immediate forms through
latency filling and list scheduling.

The main matrix has **1,470 runnable pairs**: four index sources, three base
kinds, four element types, and O0/O4 across 15 builds, plus the C++ Wind Waker
getter. Each synthetic object contains three masks, including a discontiguous
mask. Whole objects improve **0/1,470 → 657/1,470**, with every candidate
compilable. An additional 180-pair edge panel covers repeated uses, commuted
masks, a high-bit mask, and zero. It improves **0/180 → 30/180** while retaining
**150/180 compilations**; the broader expression with a second table read
still declines on all 30 configurations.

Unicorn execution validates **540 candidate/reference object pairs** with
**12,960 calls**, covering byte, halfword, and word loads, all three masks,
and four input bit patterns. Another **600 calls across 75 edge pairs** check
reused indices, commutation, high-bit scaling, and zero. All checks pass.
All **100 register-allocation/scheduler tests** pass (eight pre-existing
ignored tests), as do **45 version tests** and **eight focused driver tests**.

The original index regression selection retains **972 exact objects across
1,674 runnable pairs** (2,115 slots, 441 exclusions). The metadata selection
retains **1,179 exact objects across 2,198 runnable pairs** (2,940 slots,
742 exclusions). Neither panel loses an exact match or hits a timeout.
Every individual object and code verdict remains unchanged in the
**132-config real-project panel: 47 BYTE, 16 DIFF, 59 compiler DEFER, six
HARNESS, four missing dependencies**. Code remains **46/109 exact measured
objects**, five empty and eighteen unmeasured; all 51 measured partial
translation units remain nonexact. Complete `DynamicLink.cpp` and `GeoTree.cpp`
configurations still hit their 30-second caps, so the getter result is not a
complete-file or project-build claim. Evidence is under `target/masked-index-*`.

## Static aggregate storage and addressing, 2026-09-06

Against frozen baseline `e6cc821f`, a 360-pair C/C++ O0/O4 matrix improves
**64/360 → 256/360 whole-object exact pairs**, with all candidates compilable
and no exact losses. The matrix covers nine element types, seven array lengths,
three scalar record layouts, initialized/const/zero storage, and small/full
addressing across all 15 builds. All **23,760 symbol-alignment records** now
match, up from **19,920**. Section, size, offset, and alignment agree for
**23,628/23,760 records**; the remaining 132 are newer-build C small-BSS
placement differences.

A separate static-storage helper shares element-size calculation between
storage construction and address-mode selection. Mainline O0 local aggregates
keep natural alignment even in small data; newer builds promote nonzero total
sizes divisible by eight. This applies to scalar records as well as arrays,
without changing record member layout. Explicit alignment is retained. The
oldest four builds treat it as an override, allowing reduced alignment; later
builds treat it as a minimum. A 60-pair explicit-alignment panel improves
**0/60 → 48/60 exact objects**, with all **960 alignment records** matching
(up from 116), all candidates compilable, and no exact losses.

The new corpus also exposed a static-record-array addressing defect: array
size was calculated from the scalar type width when selecting SDA21 versus
full address relocations. It now uses the same complete record size as storage.
The optimized and O0 storage canaries 1654–1655 each improve **0/15 → 14/15**.
Canary 1656 preserves Wind Waker's `DynamicModuleControl::getModuleTypeString`
body and the documented member offset in a minimal class declaration. All
15 builds compile it, but its masked-index instruction selection remains
nonexact. Together the three new cases improve **0/45 → 28/45**. The earlier
C++ aggregate-string case 1653 retains 12/15, with newer ordinal gaps remaining.

Rechecking the preceding initializer panel improves **362/600 → 378/600 exact
objects**, preserving every previous match and all compilations. The metadata
regression panel retains **1,179 exact objects across 2,198 runnable pairs**
(2,940 selected slots, 742 exclusions), without losses or timeouts. Three new
storage-policy tests and eight focused driver tests pass.

The real-project selection now includes all four configured Wind Waker
`DynamicLink.cpp` variants. Both baseline and candidate hit their 30-second
cap, as do the two existing complete `GeoTree.cpp` configurations. Every
individual object and code verdict is unchanged in the **132-config panel:
47 BYTE, 16 DIFF, 59 compiler DEFER, six HARNESS, four missing dependencies**.
Code remains **46/109 exact measured objects**, five empty and eighteen
unmeasured; all 51 measured partial translation units remain nonexact. No
complete-file or project-build gain is claimed from the reductions. Evidence
is under `target/static-alignment-*`; the preceding matrix result is
`target/static-order-probes/static-alignment-results.json`.

## Constant static-initializer string order, 2026-09-06

Against frozen baseline `00651054`, the ten-shape C/C++ O0/O4 initializer
panel improves **80/600 → 362/600 whole-object exact pairs**. All 600 compile
on both sides, and no previous exact match is lost. New canaries 1651–1653
cover multiple declarations, fresh and reused strings, neighboring scalar
locals, and C++ array/record initializers. They improve **0/45 → 40/45**:
both C cases match 14/15 builds, while the C++ aggregate case matches 12/15.
Together with 1644 and 1647–1650, the focused permanent selection improves
**50/120 → 108/120**; all pairs are reference-runnable and compile.

A separate driver planner assigns fresh literal identities during each local
initializer and carries their consumed slots into subsequent local identities.
Reuse of an earlier literal consumes no slot. Packed and explicitly deferred
pools retain their existing numbering owners. The object writer follows these
initializer dependencies when placing `.data`, `.sdata`, and `.rodata` and
when emitting local symbols. This also handles a pointer in one section whose
literal belongs to another. Writable section anchors are emitted at the
function-owned data event, preserving earlier local function symbols.

The preceding string panel improves **660/960 → 722/960 exact objects**, with
all 960 compilable and no exact losses. The existing metadata regression panel
retains all **1,179 exact objects** across **2,198 runnable pairs** (2,940 slots,
742 build exclusions). All **40 object-writer tests** and **eight focused driver
tests** pass. The new writer test checks declaration/reuse order, physical
section offsets, and the writable anchor's position after an earlier function.

The Mario Kart Double Dash getter reduction 1647 remains **12/15**. Its newer
GameCube builds still differ in anonymous numbering around class declarations;
this change does not establish their parity. Wii also retains weak,
function-qualified static names and different local ordinal consumption.
The aggregate probes expose newer-build eight-byte alignment gaps. Older C++
multi-local scalar initialization still needs a broader guarded lowering owner.

Every individual whole-object and code verdict remains unchanged across the
**128-config real-project panel: 47 BYTE, 16 DIFF, 59 compiler DEFER, two HARNESS,
four missing dependencies**. Code remains **46/109 exact measured objects**,
five empty and fourteen unmeasured; all 51 measured partial translation units
remain nonexact. Both complete `GeoTree.cpp` configurations still hit the
30-second cap. These focused results are not a corpus or project-build parity
estimate. Evidence is under `target/static-order-*`; the preceding string
matrix is `target/string-alignment-probes/static-order-results.json`.

## Guarded C++ static pointer getters, 2026-09-06

Against frozen baseline `382a3e86`, the Mario Kart Double Dash getter reduction
1647 improves **0/15 → 12/15 whole-object exact pairs**. All twelve older
builds now match; the newest three retain constant-data symbol/string-order
differences. Three new canaries (1648–1650), covering O0, constant addresses,
byte offsets, explicit null pointers, and C linkage inside C++, improve
**2/45 → 38/45**. Combined, these four canaries improve **2/60 → 50/60**, with
all reference pairs runnable and all candidates compilable.

Reference probes establish a frontend compatibility rule: builds through
GC/2.7 use a first-use guard even for constant C++ scalar pointer initializers,
including integer casts and explicit null. GC/3.0a3 onward uses constant data.
A profile enum separates those choices and the older sequential versus
mainline scheduled instruction order. The codegen boundary now receives
explicit source-language facts; `extern "C"` functions therefore retain C++
initialization behavior without relying on mangled names.

A separate lowering owner emits the guarded getter protocol, its zero-storage
pointer and byte guard, and ordinary string/symbol relocations. O0 retains its
explicit compare and assignment ordering; optimized builds use the record-form
guard test and versioned schedules. Symbol-plus-byte-offset initializers retain
a separate address adjustment. The pool-number walk counts emitted static
objects, including the synthesized guard, so it agrees with the object writer.
This entry point requires a getter with one static pointer declaration and no
other executable work. General control-flow declaration positions, multiple
locals, far-addressed guard storage, and wider integer construction remain
outside this owner; it does not hoist arbitrary initialization to function entry.

Rechecking the preceding 960-pair string panel improves **612/960 → 660/960
whole-object exact pairs**, preserves every previous exact match, and keeps
all 960 candidate compilations successful.

The ten-shape C/C++ O0/O4 panel has **600 reference-runnable pairs**. Whole
objects improve **96/600 → 288/600**, while compilation remains **480/600**;
the two broader multi-local/write-body shapes still decline. An additional
210-pair edge panel covers explicit null, C linkage, unsigned-char mode,
read-only and packed literals, small symbol offsets, and a const scalar target.
It matches **144/210 objects**; all 30 const-scalar-target cases still fail in
the pre-existing constant-address parser. Packed strings also retain a late
scheduling mismatch on the four oldest optimized builds.

Unicorn execution checks compare **216 candidate/reference getter pairs**:
**3,024 total calls** validate initial pointer storage and guard setting, five
nonzero guard-byte values while preserving an altered pointer, and reinitialization
after clearing the guard. This validates the emitted first-use protocol in
addition to byte equality. Three focused lowering tests, two driver static-local
tests, and all **45 version tests** pass.

The existing metadata panel retains all **1,179 exact objects** across **2,198
runnable pairs** (2,940 selected slots, 742 build exclusions), with no exact
losses or timeouts. Every individual object and code verdict remains unchanged
across the **128-config real-project panel: 47 BYTE, 16 DIFF, 59 compiler DEFER,
two HARNESS, four missing dependencies**. Code remains **46/109 exact measured
objects**, five empty and fourteen unmeasured; 51 partial translation units
remain nonexact. Both complete `GeoTree.cpp` configurations still hit the
30-second cap, so matching the three reduced getter bodies does not establish
complete-file or project-build parity. Evidence is under `target/static-string-*`.

## String storage and static-local string pointers, 2026-09-06

Against frozen baseline `3542bbff`, seven new canaries (1641–1647) improve
from **28/105 to 65/105 whole-object exact pairs**, all reference-runnable.
The optimized and O0 literal-size canaries each match **15/15**, as does the
BFBB data reduction containing the 89 original file-scope string pointers from
`zNPCTypeVillager.cpp` (**12/15 → 15/15**). Packed-pool alignment and short-pool
storage each match **10/15**; the five oldest builds retain relocation-anchor
mismatches. The static-local pointer canary and three original Mario Kart
Double Dash `GeoTree.cpp` getter bodies now compile on every build, but remain
nonexact. The latter reduction substitutes minimal class declarations for the
project headers; neither reduction establishes a whole-project build result.

All five driver paths that materialize string objects now share the existing
versioned array-alignment calculation. Byte size includes the terminator, or
the complete packed pool. Mainline O0 strings retain byte packing in both
small and full sections; the newest builds promote whole sizes divisible by
eight. Packed pools always use full `.data`/`.rodata`, including pools at or
below the small-data threshold.

Static-local scalar pointer initializers now return typed relocation targets,
allowing strings to reach the existing function-owned pool without converting
raw literal bytes through text. Leading and nested local declarations both
preserve the pointer object's constness before parsing its initializer;
`static const char* p` stays writable, while `static char* const p` is const.
The parser test covers embedded NUL and non-UTF-8 bytes in both declaration paths.

The four-shape matrix spans globals, aggregate fields, function literals, and
static-local pointers; nineteen literal sizes; C/C++; O0/O4; normal, far,
read-only, and packed storage; and all fifteen builds. Among **960 reference
pairs**, compilation improves **720/960 → 960/960**, and complete objects
**348/960 → 612/960**, with no exact losses. The separate 240-pair packed-pool
size panel improves **60/240 → 156/240**. Remaining differences include static
local numbering/order and code, and older read-only/packed relocation policy.
The preceding pointer-table matrix improves **404/420 → 420/420 exact objects**.

The real-project panel preserves every individual object and code verdict
across **128 configurations: 47 BYTE, 16 DIFF, 59 compiler DEFER, two HARNESS,
and four missing dependencies**. Code remains **46/109 exact measured
objects**, five empty and fourteen unmeasured; the 51 emitted partial
translation units remain nonexact. Both actual `GeoTree.cpp` configurations
reach the 30-second cap in baseline and candidate, so their reduced getters'
new compilability is not a complete-file result.

The focused existing metadata panel retains all **1,179 exact objects** among
**2,198 runnable pairs** (2,940 selected slots, 742 build exclusions), without
exact losses or timeouts. Validation also passes **406 parser tests**, excluding
the same two documented pre-existing failures, and the driver array-alignment
matrix. Evidence, frozen binaries, and probe scripts are under
`target/string-align-*` and `target/string-alignment-probes/`.

## Const address tables and pointer-array storage, 2026-09-06

Against frozen baseline `c8f317d6`, four new canaries (1637–1640) improve
from **4/60 to 60/60 whole-object exact pairs** across all fifteen builds.
They cover C/C++ mutable, internal const, and exported const data tables;
const function-pointer typedefs; and both callback tables reduced from
Melee's `src/melee/gr/grcastle.c`. The Melee reduction preserves the table
initializers and function signatures while replacing project headers with
opaque parameter types and omitting function bodies. It improves from
**4/15 to 15/15**; this is an isolated data-object result, not a whole-file
or whole-project result. Existing mixed-BSS canary 1630 also improves from
**12/15 to 15/15** through the array-alignment fix.

Address-table classification now accepts internal or const tables targeting
declared functions and data, including external data, byte addends, and mixed
string/data slots. Unknown targets still defer. Pointer arrays reuse the
existing versioned array-alignment calculation, including the newest builds'
eight-byte promotion. A separate profile policy captures the four oldest
builds' writable placement of exported const address arrays in C and exported
const addresses in C++; C scalar const addresses remain read-only. Later
builds place these const addresses in read-only sections. The parser preserves
both prefix and postfix const on pointer aliases, including declaration
specifiers consumed before type parsing, and separates an added outer pointer.

The seven-shape table matrix spans C/C++, O0/O4, and all fifteen builds:
**420 reference-runnable pairs**, candidate compilation **60/420 → 420/420**,
and whole-object equality **0/420 → 404/420**. The remaining 16 differences
are O0 anonymous-string alignment metadata in the mixed string/data shape.
A separate scalar const-address matrix matches **60/60 objects**. Rechecking
the preceding 480-pair offset matrix raises compilation **450/480 → 480/480**
and whole-object equality **356/480 → 393/480**, with no exact losses.

The focused existing metadata panel retains **1,179 exact objects** among
**2,198 runnable pairs** (2,940 selected slots, 742 build exclusions), with
no exact losses or timeouts. The 126-configuration real-project panel retains
every individual whole-object and code verdict: **47 BYTE, 16 DIFF, 59 compiler
DEFER, and four missing dependencies**. Code remains **46/109 exact measured
objects**, five empty and twelve unmeasured; 51 emitted partial translation
units remain nonexact. This panel omits the separately measured BFBB asset-file
timeout and does not imply full-corpus or project-build parity.

Validation: **405 parser tests pass** with the same two documented pre-existing
failures excluded; **nine initializer-classification tests**, the driver array
alignment matrix, and **45 version tests** pass. Probe, frozen-baseline,
regression, and reference-panel evidence is under `target/pointer-table-*`.

## Typed pointer-offset initializers, 2026-09-06

Against frozen baseline `681ee04f`, four new canaries improve from **0/60
to 59/60 whole-object exact pairs**, across all fifteen builds. Every
reference pair is runnable; candidate compilation improves from 0/60 to
60/60. Canaries 1633/1634/1635 cover typed subscripts, arithmetic and casts,
and struct-array member addresses, each matching **15/15**. The BFBB-derived
one-past-end texture-table canary 1636 matches **14/15**; GC/1.3 retains a
section/symbol mismatch despite exact text.

Pointer initializers now use the ordinary expression parser and a separate
constant-address evaluator. Existing type-size queries supply element strides;
cast placement determines subsequent scaling, while an already computed
address retains its byte displacement. The AST carries a named symbol plus a
signed byte addend, preserving the old zero-addend representation. This reaches
global pointer/flat address-table images and static-local relocation images.
Leading function-scope pointer declarations now use the same serializer as
nested static declarations. Writable static scalar pointers retain local
linkage, and offset references participate in registration-object liveness.

The 16-shape C/C++ matrix has **480 reference-runnable pairs**. Candidate
compilation improves from **0/480 to 450/480**, and complete objects from
**0/480 to 356/480** (356/450 among candidate-compilable pairs). Complete
relocation-record sets match **418/450** emitted pairs. Typed char/short/int/
double indexing, arithmetic, negative offsets, and struct-member shapes each
match **30/30 whole objects** across languages and builds. The remaining 30
compiler declines concern static/const pointer-address tables; emitted-object
differences include static-local conventions and other ordering/layout gaps.

The two texture declarations copied directly from BFBB's `zAssetTypes.cpp`
produce **15/15 exact data-only objects** with the common probe flags. This is
an isolated source reduction, not a whole-project result. The complete source
still reaches the 30-second cap in both baseline and candidate. The combined
real-project panel retains every object and code verdict across **127
configurations: 47 BYTE, 16 DIFF, 59 compiler DEFER, one HARNESS, and four
missing dependencies**. Code remains **46/109 exact measured objects**, five
empty and 13 unmeasured; the 109 include 51 nonexact partial-TU projections.
Melee's complete transport object remains exact.

The expanded metadata/pointer/address/initializer regression selection retains
**1,179/2,198 exact runnable pairs**, across **2,940 slots, 742 exclusions,
and zero timeouts**, with no lost matches. **404 parser tests pass**, including
three new typed-offset/static-local/runtime-rejection tests; two previously
recorded failures remain in friend/expression-template recovery and discarded
inline aggregate-image accounting. The full run reproduced those two failures;
the subsequent run excludes them. All **eight initializer-driver tests pass**,
including offset-reference liveness coverage. Compiler and oracle builds pass.

Local evidence: `target/pointer-offset-*`, `target/probe_pointer_offsets*.py`,
`target/probe_real_texture_end.py`, `target/check_pointer_offset*.py`, and
`target/reference-parity/73acd82ec9b699ee-5e4ca1ddc460f4d8.jsonl`.

## C++ zero-storage declaration order, 2026-09-06

Against frozen baseline `b9f4933a`, four targeted canaries improve from
**10/60 to 57/60 whole-object exact pairs** across all fifteen builds.
Exported-BSS canary 1629 now matches **15/15**, up from zero. New canaries
1630/1631/1632 cover mixed static/exported BSS, declarations around function
definitions, and initializer references before and after a definition. They
improve from **10/45 to 42/45**. All 60 pairs compile without exclusions or
reference rejections; 1630 retains three modern pointer-array alignment gaps.

C++ now allocates full BSS in source declaration order across static and
exported definitions, and emits exported BSS symbols at their declaration
events alongside other globals. Function-local statics, appended separately
to the writer input, rejoin the sequence at their owning function's source
position. A shared stable ordering feeds both `.bss` and `.sbss`. The format
policy is renamed `zero_data_in_declaration_order` to describe its full scope;
C's independent placement and reference-order conventions remain separate.

Initializer section anchors also respect definition order. A reference to a
preceding extern declaration keeps its named relocation until storage is
defined. A later initializer can use the same target's BSS anchor. Eligibility
is resolved per initializer, so both relocation forms can coexist in one
object without retargeting the earlier reference.

Ten source shapes across fifteen builds and three modes (O4, O0, and O4 with
`-sdata 0`) improve from **30/450 to 375/450 exact objects**, with every pair
compilable and no lost matches. Symbol-name sequences improve from **103/450
to 435/450**; complete symbol-record sets from **45/450 to 396/450**; relocation
record sets from **337/450 to 428/450**. Both forward-reference forms match
all 45 mode/build pairs each. Remaining nonexact cases expose explicit-zero
section selection, pointer-array alignment, function-local static conventions,
and code scheduling. These are targeted diagnostics, not population estimates.

All **39 writer tests pass**, including mixed exported/body-local zero storage
and a single target used by both pre-definition and post-definition
initializers. Compiler and oracle builds pass. The fixed metadata selection
retains **908/1,750 exact runnable pairs**, with **2,250 slots, 500 exclusions,
and no timeouts**.

The combined **191-configuration** real-project panel retains all **47 exact
objects** and all previously measured code verdicts. Candidate results are
**47 BYTE, 22 DIFF, 105 compiler DEFER, 13 HARNESS, and four missing dependencies**.
Code is exact for **47/156 measured objects**, with five empty and 30 unmeasured;
92 of the measured objects are nonexact partial-TU projections. The baseline
has 104 DEFER and 14 HARNESS: Wind Waker's `m_Do_DVDError.cpp` reaches a long-long
codegen diagnostic in the candidate instead of the 30-second cap. That adds
one nonexact partial projection and is not counted as a parity gain. An
isolated baseline rerun reaches the same diagnostic and code verdict, confirming
timing variability at this cap. The transport subset retains **36/40 exact objects**, including Melee, with four
missing dependencies and **34/34 exact nonempty code projections** plus two
empty objects.

Local evidence: `target/cxx-bss-*`, `target/check_cxx_bss*.py`,
`target/probe_cxx_bss_declarations.py`, and
`target/reference-parity/a1aaaed17c7a07db-5e4ca1ddc460f4d8.jsonl`.

## BSS initializer section anchors, 2026-09-06

Against frozen baseline `b04e2202`, the targeted set improves from **20/60
to 34/60 whole-object exact pairs** across all fifteen builds. Canary 1625
now matches **15/15**, closing its five older-build initializer-relocation
gaps. New C/C++ local-BSS canaries 1627/1628 improve from **0/15 to 4/15**
and **10/15 to 15/15**, respectively. New exported-C++ canary 1629 remains
**0/15**, retaining the separate global symbol-order and storage-layout gap.
All 60 pairs compile, with no exclusions or reference rejections.

The writer now gives declaration-allocated BSS definitions the older builds'
initializer section-anchor behavior. Legacy C statics qualify; C++ also
includes exported definitions through GC/1.3. C tentative globals retain
named relocations. The existing version and language policies determine
eligibility without introducing build-name checks in the writer.

The shared anchor emitter handles source declaration positions, existing
code-reference creation points, and captured symbol order. Initializer
relocations carry the target object's section offset plus their own addend.
GC/1.3's initializer anchor records the measured section-anchor comment flag;
legacy builds keep zero flags. Unit tests cover front/middle/tail creation,
local versus exported eligibility, addend composition, and comment metadata.
All **37 writer tests pass**, and compiler/oracle builds pass.

Ten source shapes across C/C++ and fifteen builds supply **300 reference
pairs, 270 candidate-compilable and 30 existing compiler declines**. Exact
objects improve from **101/270 to 153/270**, with no lost matches. Complete
relocation-record sets improve from **175/270 to 242/270**, and symbol-name
sequences from **122/270 to 174/270**. Remaining differences include C++ full
BSS global layout/order, C initializer first-use symbol events, and other
section-anchor creation positions. Const pointer-address definitions cause
the 30 declines. Separate reference probes preserve nonzero indexed and
arithmetic initializer addresses; the current parser declines those forms,
so the paired writer panel uses zero-index addresses into distinct objects.

The fixed metadata regression selection retains **908/1,750 exact runnable
pairs**, with every verdict unchanged across **2,250 slots, 500 exclusions,
and zero timeouts**. The combined real-project metadata/transport panel
retains every object and code verdict across **126 configurations: 47 BYTE,
16 DIFF, 59 compiler DEFER, and four missing dependencies**, with no harness
failures. Code remains **46/109 exact measured objects**, five empty and 12
unmeasured; the 109 include 51 nonexact partial-TU diagnostic projections.
The transport subset still has **36/40 exact complete objects**, including
Melee, and **34/34 exact nonempty code projections** plus two empty objects.

Local evidence: `target/bss-anchor-*`, `target/check_bss_anchor*.py`,
`target/probe_bss_initializer_anchors.py`, and
`target/reference-parity/e612c58ab68d7f5f-5e4ca1ddc460f4d8.jsonl`.

## C++ zero-static declaration events, 2026-09-06

Against frozen baseline `8c929c5f`, three new canaries improve from **6/45
to 35/45 whole-object exact pairs**, across all fifteen builds with no
exclusions or reference rejections. Data-only canary 1624 improves from
4/15 to 15/15; initializer references in 1625 improve from 0/15 to 10/15;
string/declaration frontiers in 1626 improve from 2/15 to 10/15.

C++ now selects the writer's existing source-declaration policy for local
data symbols on every build. The old tentative-zero phase depended on the
first function, omitting those symbols entirely in data-only objects and
misordering them around explicit-zero definitions and function-owned pools.
Removing that duplicate phase also lets initializer relocations resolve to
the defined local symbols. Physical small-data layout, early static-function
prototypes, and owned RTTI capture policies retain their separate controls.

The C++ array matrix now contains **10,500/10,500 reference symbols with
matching alignment records**, restoring all 770 previously missing symbols.
This is symbol/alignment evidence, not a whole-object count. Four targeted
source shapes improve from **8/60 to 40/60 exact objects**. Six additional
function/string/float/declaration shapes improve from **18/90 to 78/90**;
all 90 symbol-name sequences match. No previously exact probe regresses.
The five older-build failures in canary 1625 still need full-BSS initializer
section anchors. Canary 1626 retains existing code scheduling differences
on GC/1.1, GC/1.2.5, and the three newest builds.

All **35 object-writer tests pass**, including a data-only test that checks
local binding, section identity, and initializer relocation resolution.
The fixed metadata regression selection retains **908/1,750 exact runnable
pairs**, with every verdict unchanged across **2,250 slots, 500 exclusions,
and zero timeouts**. Compiler and oracle builds pass.

A new local-project diagnostic selection takes the 65 smallest existing C++
sources containing a tentative static declaration, after deduplicating by
project/build/basename. Its paired object and code verdicts are unchanged:
**0 BYTE, 6 DIFF, 45 compiler DEFER, and 14 HARNESS**. Eleven harness results
hit the 30-second cap; three are other harness failures. Code is exact for
**1/46 measured objects**, with 19 unmeasured; the 46 include 40 partial-TU
projections, none exact. Metroid Prime's `IWeaponRenderer.cpp` retains its
exact executable projection but differs in the complete object. This is a
failure-biased diagnostic panel, not a representative parity estimate.

The established transport panel retains **36/40 byte-identical objects**,
four missing dependencies, and **34/34 exact nonempty code projections**
plus two empty objects. Melee's complete transport object remains exact.

Local evidence: `target/cxx-zero-*`, `target/check_cxx_zero*.py`,
`target/probe_cxx_zero*.py`, `target/array-align-probes/cxx-alignments.json`,
and `target/reference-parity/9c6665f39ff8a092-5e4ca1ddc460f4d8.jsonl`.

## Array object alignment, 2026-09-06

Against frozen baseline `d6867f32`, the targeted set improves from **24/45
to 45/45 whole-object exact pairs**. Canary 1618 now matches all fifteen
builds, closing its unused array alignment gap. New optimized/O0 canaries
**1622/1623 improve from 12/30 to 30/30 exact pairs**; every pair compiles,
with no exclusions or reference rejections.

The discrepancy involved storage offsets as well as `.comment` metadata.
The new `ArrayAlignmentStyle` profile separates three measured conventions:
legacy builds retain a word minimum even at O0; intermediate builds use
natural array alignment at O0, except arithmetic arrays in small data keep
word-aligned storage while recording element alignment; newest builds align
arrays whose nonzero byte size is divisible by eight to at least eight bytes,
with a word minimum otherwise. Explicit alignment requests remain lower
bounds. The rule covers read-only and writable arrays and struct elements;
scalar aggregate conventions remain separate.

A named alignment input now carries element size/alignment, array extent,
read-only state, requested alignment, optimization, and small-data mode.
Storage and metadata remain distinct outputs. Promotion uses the whole array
size, rather than confusing element width with object size or applying the
large scalar-aggregate metadata rule to every array.

**10,500/10,500 C scalar-array alignment records match**, across five element
types, seven extents, storage/initialization variants, two optimization levels,
and all fifteen builds. C++ contributes another **9,730/9,730 comparable
alignment records**; its other 770 reference symbols are absent from candidate
data-only objects, identically in the frozen baseline. That existing C++
zero-static emission gap remains outstanding. These record counts measure
alignment, not complete-object parity.

Storage-offset and explicit-alignment probes match all **60/60 full symbol
record sets** across fifteen builds and O0/O4. Another **12/12 sets match
with `-sdata 0`** on GC/1.3, GC/2.7, and Wii/1.0. Struct-array edge probes
confirm the same array rules; scalar-struct alignment differences remain
outside this change. The new canaries cover sizes 8, 9, and 16, mixed scalar
padding, read-only arrays, and an explicit 32-byte alignment.

The driver alignment matrix and all **45 version tests pass**, including a
new family-policy test. The metadata regression selection improves from
**899 to 908 exact objects**, with no regressions: **2,250 slots, 500
exclusions, 1,750 runnable pairs**, and no timeouts. The nine gains are
canaries 701, 741, and 1230 on GC/3.0a3, GC/3.0a3p1, and Wii/1.0.

All 86 real-project metadata configurations retain their object and code
verdicts: **11 BYTE, 16 DIFF, and 59 compiler DEFER**, with no harness or
dependency failures. Code projections remain 12/75 exact, three empty,
and eight unmeasured, including 51 measured partial-TU diagnostics. The
transport panel retains **36/40 byte-identical objects**, four missing
dependencies, 34/34 exact nonempty code projections, and two empty objects.
Melee's complete transport object remains byte-identical.

Local evidence: `target/array-align-*`, `target/check_array_align*.py`,
`target/probe_array_comment_alignment*.py`, `target/probe_array_alignment*.py`,
and `target/reference-parity/d4daf8e25b284d81-5e4ca1ddc460f4d8.jsonl`.

## Zero-static first-use transactions, 2026-09-06

Against frozen baseline `a32fb7c1`, six targeted samples improve from
**21/90 to 48/90 whole-object exact pairs**, with no regressions. The gains
are eight additional builds each for canaries 1618 and 1619, plus eleven
for new canary 1620. All 90 pairs compile without exclusions or reference
rejections. Packet-read canaries 1614/1615 retain their eight legacy matches.

Immediate C compilation now creates an uninitialized file static inside the
function that first references it, instead of grouping every such symbol at
the front of the object. Unused definitions finish in reverse declaration
order after the function stream. Existing declaration-order, deferred/C++,
and explicitly captured symbol-creation policies retain their owners.

The physical symbol phase follows the existing function-order policy and
object storage class. Reference-first builds resolve the symbol before the
function; function-first builds resolve small-data statics after it. Full
`.bss` definitions still precede that function event. First-use references
also retain their measured position before or after newly discovered string
symbols. A named phase separates these decisions from symbol serialization;
relocation order supplies the discovery stream, including hoisted addresses.

All **225/225 symbol sequences match** for the fifteen declaration/order
probes across all fifteen builds. Six additional storage/address/string
probes produce **60/60 matching symbol sequences** where the candidate
compiles. Their other 30 pairs hit an existing mixed scalar/dereference
allocator rejection; the reference still supplies ordering evidence for
those cases. These are symbol-order diagnostics, not whole-object claims.

New canary **1620 matches 15/15 whole objects**, up from 4/15, and separates
first uses across functions with intervening code and unused definitions.
New canary **1621 retains 3/15 exact objects**, exercising a string-bearing
full-BSS read followed by a small-data read; other builds retain code gaps.
Canaries 1618 and 1619 now match 12/15 and 10/15 objects respectively. The
newest three builds' remaining 1618 difference is the `.comment` alignment
of its unused full-BSS array (reference 8, candidate 4); executable code and
symbol entries agree. That metadata gap remains outstanding.

All **34 object-writer tests pass**, including two new tests for first-use
ownership, unused-tail order, the full-BSS/small-data split, string phases,
and relocation symbol identities. The metadata regression set retains every
verdict: **2,250 slots, 500 exclusions, 1,750 runnable pairs, 899 exact
objects**, with no timeouts.

The 86 real-project metadata configurations retain every object and code
verdict: **11 BYTE, 16 DIFF, and 59 compiler DEFER**, with no harness or
dependency failures. Their code projections remain 12/75 exact, three
empty, and eight unmeasured, including 51 measured partial-TU diagnostics.
The transport panel retains **36/40 byte-identical objects**, four missing
dependencies, 34/34 exact nonempty code projections, and two empty objects.
Melee's complete transport object remains byte-identical.

Local evidence: `target/zero-reference-*`, `target/check_zero_reference*.py`,
`target/probe_zero_static_order.py`, `target/probe_zero_reference_edges.py`,
`target/inspect_zero_reference_canaries.py`, and
`target/reference-parity/4bc2052621eecb1a-5e4ca1ddc460f4d8.jsonl`.

## File-static declaration frontiers, 2026-09-06

Against frozen baseline `b2c7d826`, the remaining legacy symbol-order gap in
packet-read canaries **1614/1615 is closed**: their executable-code matches
now become **8/30 whole-object exact pairs**, up from 0/30. Both objects match
all four legacy builds. The other 22 pairs still have code differences.

The writer's existing declaration-order profile now applies consistently to
zero-filled file statics as well as initialized ones. Such objects enter the
local symbol stream at their declaration frontier, before the next function's
strings and locals, regardless of first use. They no longer fall through to
the separate first-reference path. A shared declaration-event emitter handles
both inter-function positions and the end of the function stream.

The driver also preserves tail declaration positions instead of moving all
plain tail statics to the front. ELF's local-before-global symbol grouping
already explains the apparent early placement when preceding functions are
global. Moving the declaration itself wrongly crossed earlier static
functions and anonymous data. Section-attributed objects and explicit captured
creation events retain their existing owners.

Fifteen declaration-order probes were compiled across all 15 builds; all
**60/60 legacy symbol sequences match**. They cover leading, interleaved,
delayed-use, unused, and tail declarations; mixed initialized/zero objects;
strings before and after declarations; constant pointers; and large `.bss`
arrays. The probes also establish the outstanding newer-build distinction:
ordinary uninitialized statics follow first use, with different placement
relative to function symbols. That reference timeline remains future work.

New canaries **1618/1619 improve from 0/30 to 6/30 whole-object exact pairs**.
The declaration-frontier sample matches all four legacy builds; the string/tail
sample matches GC/1.1p1 and GC/1.2.5n. Its GC/1.1 and GC/1.2.5 differences are
an existing return-load/epilogue schedule gap. All 30 compile, with no exclusions
or reference rejections. Together with 1614/1615, the targeted set improves
from **0/60 to 14/60 exact objects**.

All **32 object-writer tests pass**, including a new matrix proving front,
middle, tail, and collapsed source positions with used/unused objects and
checking relocation symbol identities after reordering. The focused metadata
regression selection retains every verdict: **2,250 slots, 500 exclusions,
1,750 runnable pairs, 899 exact objects**, and no timeouts.

An additional **86 real-project metadata configurations retain every object
and code verdict**: 11 BYTE, 16 DIFF, and 59 compiler DEFER, with no harness or
dependency failures. Code projections retain 12/75 exact, three empty, and
eight unmeasured; 51 measured projections are partial-TU diagnostics. The
40-configuration transport panel retains **36 BYTE and four missing
dependencies**, with **34/34 nonempty code projections exact** and two empty.
Melee's complete transport object remains byte-identical.

Local evidence: `target/zero-static-*`, `target/check_zero_static*.py`,
`target/probe_zero_static_order.py`, and
`target/reference-parity/9a7dd2803d124bf4-5e4ca1ddc460f4d8.jsonl`.

## Complete Melee transport matching, 2026-09-06

Against frozen baseline `c37dff14`, Melee's `DBWrite` now matches all **608
reference function bytes**. The complete `odenotstub.c` object is now
**byte-identical**, improving from **20/21 to 21/21 exact functions** and
**2712/3320 to 3320/3320 exact reference function bytes**. This completes
this configured transport translation unit, not the wider project or corpus.

A separate source proof describes the interrupt-protected three-phase
status/stream/mailbox protocol. It derives counter selection, stream commands,
length rounding, mailbox fields, retry conditions, and the return value from
the tree. Source-visible status and mailbox definitions reuse the existing
fixed-bank proof and must share their selected/poll bank configuration. Shared
source expansion and bank checks now live with the common transaction owner.
ABI, storage, visibility, automatic inlining, optimization, and legacy profile
checks gate composition; no source names or device addresses select it.

The physical schedule retains the token and bank addresses across calls and
reuses dead stream argument registers for later phases. The first two status
phases discard transfer failures while the last retries either failure before
checking the busy bit. The reference's unused first-transfer normalization
survives. Distinct inlined command slots share one caller output word, and
GC/1.1p1 uses the existing patched linkage policy for its smaller frame and
restore order. Recognition, helper composition, and physical scheduling are
separate modules.

New canaries **1616 and 1617 improve from 0/30 to 8/30 whole-object exact
pairs**: both match all four legacy builds. All 30 pairs compile, with no
exclusions or reference rejections; the other 22 remain nonexact. The variant
changes typedefs, names, bank address and slots, selection and poll masks,
status and mailbox commands, counter selection, length rounding, message
fields, and the return value.

**9,024 paired Unicorn cases pass**: 8,256 across both canaries and all 15
builds, plus 768 using Melee's actual caller and status/mailbox helper bodies.
Checks cover ignored and retried transfer failures, noncanonical true values,
all three busy phases, repeated MMIO polling, counter and size wraparound,
command storage overwritten by callbacks, counter and bank changes across
calls, retained stream/mailbox arguments, output identity, byte/buffer guards,
interrupt-token restoration, and ABI-clobbered registers. Transfer, stream,
and interrupt primitives are modeled; this does not claim hardware execution.

Sixteen fixed-bank tests (four new source-proof tests) and 117 inline-expansion
tests pass. The previously documented embedded-asm composition failure remains
explicitly skipped. The focused regression selection retains every verdict:
**1,040 slots, 281 exclusions, 759 runnable pairs, and 417 exact objects**, with
no timeouts. Another **300 neighboring pairs retain every verdict and 133
exact objects**. All 30 new candidate objects remained byte-identical after
the final shared-helper relocation within the implementation.

The 40 real-project transport configurations improve from **35 BYTE, one DIFF,
and four missing dependencies** to **36 BYTE, zero DIFF, and four missing
dependencies**. All **34/34 measured nonempty code projections** match; two
configurations are empty and four remain unmeasured. The interleaved static
symbol-order gap in canaries 1614/1615 remains outstanding.

Local evidence: `target/bank-retry-*`, `target/check_bank_retry*.py`,
`target/probe_bank_retries.py`, `target/inspect_bank_retry_objects.py`, and
`target/reference-parity/64f5ec4c522c918c-5e4ca1ddc460f4d8.jsonl`.

## Composed packet bank reads, 2026-09-06

Against frozen baseline `a1359473`, Melee's `CheckMailBox` now matches all
**336 reference function bytes**. Its transport translation unit improves
from **19/21 to 20/21 exact functions** and **2376/3320 to 2712/3320 exact
reference function bytes**. All 21 functions compile. `DBWrite`, at 608
reference bytes, is the sole remaining nonexact function; the already exact
`DBQueryData` retains its read-helper calls.

The packet owner now composes source-visible read definitions through the
existing fixed-bank transaction proof. Both transactions must agree on the
bank, selected/poll slots, selection/reset masks, and poll predicate before
sharing saved address registers. Command tags and transfer symbols retain
independent ownership. ABI checks are shared with standalone transactions.
The read proof also rejects assembly bodies and local/parameter bindings
that shadow the fixed bank. No source names or device addresses select the
implementation.

The legacy schedule uses one 64-byte frame, distinct command slots, and a
shared packet array. It reuses selection and polling fragments and the
existing packet bitfield publication emitter. It preserves the reference's
otherwise unused first-transfer failure normalization while discarding the
second result, and follows the existing packet version policy for restores.
Automatic inlining and source visibility gate this composition; the outer
interrupt query remains a separate measured composition boundary.

New canaries **1614 and 1615 improve from 0/30 to 8/30 executable-code and
relocation-aware exact pairs**, covering both objects on all four legacy
builds. All 30 pairs compile, with no exclusions or reference rejections.
The other 22 pairs remain code-nonexact. The variant changes source typedefs,
names, bank address, selected/poll slots, masks, command tags, readiness bit,
published field, and flag value.

**Whole-object exact remains 0/30** for these samples. Their remaining legacy
difference is symbol order: zero-initialized file statics declared between
functions appear after the preceding static functions in the reference,
but the candidate's pending-zero-static phase emits them up front. Symbol
contents and code agree; this is an outstanding object-writer gap, not a
whole-object match claim.

**88,992 paired Unicorn cases pass**: 81,216 across the two canaries and all
15 builds, plus 7,776 using Melee's actual packet-helper/query bodies and
read callees. Checks cover command words and order, packet identity and
untouched words, readiness/tag branches, MMIO reads/writes, repeated polling,
transfer failures and noncanonical true values, bank changes across calls,
ABI-clobbered registers, token restoration, stack/saved registers, byte-store
guards, and post-release result reloads. Transfer and interrupt primitives
are modeled; this does not claim full hardware transport execution.

Twelve fixed-bank tests, five packet/composition tests (three new tests in
total), and 117 inline-expansion tests pass. The previously documented
embedded-asm composition failure remains explicitly skipped. The focused
regression selection retains all verdicts: **1,040 slots, 281 exclusions,
759 runnable pairs, and 417 exact objects**, with no timeouts. Another
**270 neighboring pairs retain all verdicts and 133 exact objects**.

The 40 real-project transport configurations retain **35 BYTE, one DIFF,
and four missing dependencies**; **33/34 measured code-exact**, two empty,
and four unmeasured. Melee's whole object remains nonexact.

Local evidence: `target/packet-read-*`, `target/check_packet_read*.py`,
`target/probe_packet_reads.py`, `target/inspect_packet_read_objects.py`, and
`target/reference-parity/957289bf6e89e5f5-5e4ca1ddc460f4d8.jsonl`.

## Guarded packet query matching, 2026-09-06

Against frozen baseline `7b70119c`, Melee's `DBQueryData` now matches all
**156 reference function bytes**. Its transport translation unit improves
from **18/21 to 19/21 exact functions** and **2220/3320 to 2376/3320 exact
reference function bytes**. All 21 functions compile. `CheckMailBox` and
`DBWrite` remain nonexact because their reference bodies compose additional
status/mailbox transactions.

A shared semantic description recognizes a two-word automatic packet read,
a readiness guard, a masked packet update, a tag guard, and publication of
the whole word, one bitfield, and a byte flag. Names, array index, masks,
flag value, and callees come from the source. The automatic inliner admits
this bounded helper while preserving its existing single-use, visibility,
and nesting policies. Alpha-renamed packet storage prevents collisions;
caller bindings that would capture published globals or callees decline.
The general scalar-local size threshold is unchanged.

The legacy physical owner uses that same description for the helper and
its interrupt-protected query. It retains the tested packet word through
publication, uses a 16-byte helper frame and 24-byte query frame, and leaves
the query's source token uninitialized on the bypass path. The
`PacketPublicationStyle` policy separates the original late result load,
Nintendo's early result load, and GC/1.1p1's extra token copy and restored
stack LR load. ABI declarations, scalar global types, volatility, small-data
addressing, masks, storage, and optimization settings gate admission.
Other builds continue through structured lowering.

New canaries **1612 and 1613 improve from 0/30 to 8/30 whole-object exact
pairs** across all 15 builds. Both objects match on all four legacy builds;
the other 22 pairs remain nonexact. All 30 compile, with no exclusions or
reference rejections. The variant changes source typedefs, names, packet
index, readiness bit, preserved and tested masks, published bitfield, and
flag value.

**106,272 paired Unicorn cases pass**: 103,680 across both canaries and all
15 builds, plus 2,592 using Melee's actual query body. Checks cover ordered
calls and global writes, packet identity and untouched words, byte-store
guards, callback-clobbered volatile registers, token restoration, stack and
saved registers, and result reloads after a state-mutating release callback.
They also compare the measured incoming-r31 behavior on the source's
uninitialized bypass path. Read/acquire/release calls are modeled; this does
not claim full hardware transport execution.

Four new semantic/composition tests, the profile-family test, and 117
inline-expansion tests pass. The previously documented embedded-asm
composition failure remains explicitly skipped. The focused regression
selection retains identical verdicts: **1,040 slots, 281 exclusions, 759
runnable pairs, and 417 exact objects**, with no timeouts. Another **240
neighboring pairs retain all verdicts and 125 exact objects**.

The 40 real-project transport configurations retain **35 BYTE, one DIFF,
and four missing dependencies**; **33/34 measured code-exact**, two empty,
and four unmeasured. Melee's whole object remains nonexact.

Local evidence: `target/mailbox-inline-*`,
`target/check_mailbox_inline*.py`, `target/probe_mailbox_inline.py`, and
`target/reference-parity/6d0f22279be70554-5e4ca1ddc460f4d8.jsonl`.

## Saved-call-token wrapper matching, 2026-09-06

Against frozen baseline `c44aee37`, Melee's `DBInitComm` and `DBRead` now match
all **120 and 140 reference function bytes**, respectively. The translation
unit improves from **16/21 to 18/21 exact functions** and **1960/3320 to
2220/3320 exact reference function bytes**, with all 21 functions compiling.
The three remaining nonexact functions are `CheckMailBox`, `DBQueryData`,
and `DBWrite`.

A structured saved-home plan recognizes an entry call result retained until
a final release call, with two surviving incoming arguments. The immutable
scalar token gets the highest saved register; parameter homes, prologue
order, frame slots, and restores share that role assignment. Early uses,
escapes, assignments, exits, and incompatible token storage decline. The
ordinary expression and statement emitters continue to generate the body.

A separate physical scheduler fills the measured token-capture and outgoing
argument latency slots. It preserves relocation and branch ownership while
permuting verified instruction packets. `SavedCallTokenStyle` selects the
legacy schedules: GC/1.1 and GC/1.2.5 restore LR before the final saved GPR;
GC/1.2.5n restores it last; GC/1.1p1 uses a 24-byte frame and earlier argument
materialization. The other legacy frames use 32 bytes. Compact-frame
admission proves there are no outgoing stack arguments or scratch accesses
that could overlap the saved slots. Profile selection, semantic planning,
and physical scheduling are separate; no project names or device addresses
select the implementation.

New canaries **1610 and 1611 improve from 0/30 to 8/30 whole-object exact
pairs** across all 15 builds. Both objects match on all four legacy builds;
the remaining 22 pairs still use structured lowering and remain nonexact.
All 30 compile, with no exclusions or reference rejections. The variant
changes source typedefs, names, bank address and register index, interrupt
mask, tested mailbox bit, selected offset, command base, length rounding,
and the constant return value.

**65,720 paired Unicorn cases pass**: 63,600 across the two canaries and all
15 builds, plus 2,120 using Melee's actual initialization/read wrapper bodies.
ABI-clobbering call models check interrupt-token restoration, published
pointers and callbacks, command and rounded-length arguments, state resets,
ordered writes and calls, buffer guards, return values, and saved registers.
Inputs include high-bit and all-ones words, size-rounding wraparound, both
mailbox branches, and varied initial state. The transfer calls are modeled;
this does not claim complete hardware transport execution.

Five new planning/scheduling tests, the profile-family test, and 117
inline-expansion tests pass. The previously documented embedded-asm
composition failure remains explicitly skipped. The 208-canary regression
selection retains identical verdicts: **1,040 slots, 281 exclusions, 759
runnable pairs, 417 exact objects, and 223 existing candidate rejections**,
with no reference rejections or timeouts. Another **210 neighboring pairs
retain all verdicts and 117 exact objects**.

The 40 real-project transport configurations retain **35 BYTE, one DIFF,
and four missing dependencies**; **33/34 measured code-exact**, two empty,
and four unmeasured. The gain is in Melee's function-level comparison; its
whole object remains nonexact.

Local evidence: `target/token-frame-*`, `target/check_token_frame*.py`,
`target/probe_token_frames.py`, and
`target/reference-parity/d6395ae4f816b994-5e4ca1ddc460f4d8.jsonl`.

## Saved-register frame correctness, 2026-09-06

Inspection of Melee's remaining `DBWrite` mismatch exposed an ABI failure in
baseline `a8f7329a`: the candidate saved r31 at frame offset 24 and r30 at 20,
then restored r30 from 24 and r31 from 20. Paired execution confirms that
Melee's actual caller and the reduced transport caller corrupt both incoming
registers on return. The reduced case fails on all four legacy builds.

The final linkage-first frame-order pass previously sorted adjacent saves
and restores independently over the entire instruction stream. Interleaved
entry copies split the save windows differently from the restores. The same
pass also rewrote ordinary body stores: opposite two-word packet assignments
became identical code, corrupting the reversed packet on all four legacy
builds. This is candidate miscompilation, not reference behavior.

Save scheduling now proves a complete set of unique individual frame slots
before the first control transfer, confines scheduling to that entry region,
and assigns restores from the resulting physical slot map. It declines
incomplete, duplicate, mixed multiple-register, and relocatable entry layouts.
Ordinary body packet stores and loads are outside its scheduling scope.
The existing frame-convention owner carries this shared rule; neither the
transport names nor particular register permutations select the fix.

New canaries **1608 and 1609** retain **30/30 compiling pairs** across all 15
builds, with **0/30 whole-object exact**, no exclusions, and no reference
rejections. Their exact schedules remain unfinished. The corpus now covers
the observed transport ABI failure and the independent packet-value failure.

**58,932 paired Unicorn cases pass**: 51,030 reduced transport caller cases,
4,500 two-direction packet cases, and 3,402 cases using Melee's actual
`DBWrite` bodies. Call models clobber volatile registers and check command
arguments, busy polling, retries, counter wraparound, interrupt-state tokens,
packet order, results, stack restoration, and all saved GPRs. The Melee
comparison models transfer calls in the inlined reference transactions and
out-of-line helpers in the candidate; it is not a complete hardware execution.
Frozen-baseline runs reproduce both failures before applying the fix.

All **30 frame-convention tests** pass. The new permutation test covers
**9,216 save/copy/restore layouts**, each with two exit orders; additional
tests protect body packets and reject unproven frame layouts. The 117
inline-expansion tests pass with the previously documented embedded-asm
composition failure explicitly skipped.

The 208-canary regression selection retains identical verdicts: **1,040
slots, 281 exclusions, 759 runnable pairs, 417 exact objects, and 223 existing
candidate rejections**, with no reference rejections or timeouts. Another
**180 neighboring pairs retain all verdicts and 117 exact objects**. The
40 real-project transport configurations retain **35 BYTE, one DIFF, and
four missing dependencies**; **33/34 measured code-exact**, two empty, and
four unmeasured. Melee retains **16/21 exact functions** and **1960/3320 exact
reference function bytes**. This milestone fixes execution without claiming
an additional exact object or real-project configuration.

Local evidence: `target/frame-order-*`, `target/check_frame_order*.py`, and
`target/reference-parity/0c5e63e6e675fd2a-5e4ca1ddc460f4d8.jsonl`.

## Mainline fixed-bank word streams, 2026-09-06

Against frozen baseline `7569218c`, the Melee-derived stream canaries **1604
and 1605 improve from 8/30 to 30/30 whole-object exact pairs**. New canaries
**1606 and 1607 also improve from 8/30 to 30/30**, covering a preservation
mask with bit 15 set and `-use_lmw_stmw on`. Combined, **16/60 becomes 60/60**
across all 15 supported compiler builds, with all pairs compiling and no
exclusions or reference rejections. This is a targeted diagnostic, not a
corpus-wide parity estimate.

The existing semantic transaction recognizer feeds a separate mainline
emitter. `FixedBankStreamStyle` selects the measured frame, address lifetime,
and reset schedule; no source names or device addresses select a template.
Mainline frames use 32 bytes and individually save r28-r31 even when multiple
register saves are enabled. GC/1.3 materializes reset masks in registers,
including the distinct unsigned-high-mask schedule. Later 2.4 builds use an
immediate reset and materialize the polling slot for writes. 4.x retains the
bank page through the first call and poll; Wii advances the write-frame store
and reuses the existing polling-alignment policy. Checked displacement
admission prevents folded addresses from overflowing their signed fields.
Legacy single-word mailbox admission and emission remain unchanged.

For canary 1604, reference read/write function sizes are respectively
**252/252 bytes** on GC/1.3, **248/256** on later 2.4, **240/240** on GC/3.0a3,
and **244/248** on Wii/1.0. Both functions now match each corresponding size,
instruction stream, and relocation sequence.

**413,568 paired Unicorn cases pass**: 400,896 detailed cases across GC/1.3,
GC/1.3.2, GC/3.0a3, and Wii/1.0 for all four canaries, plus 12,672 smaller
checks across the other eleven builds. An ABI-clobbering transfer model
compares command/data words, buffer guards, device event order, stack and
saved registers, and results. It includes negative/zero lengths, partial
words, repeated iterations through 32 bytes, varied polling delays, and
failures at each call position. These execute compiled caller bodies with
a modeled transfer, not the complete hardware transport.

The 208-canary regression selection retains identical verdicts: **1,040
slots, 281 exclusions, 759 runnable pairs, 417 exact objects, and 223 existing
candidate rejections**, with no reference rejections or timeouts. Another
**120 neighboring pairs retain all verdicts and 57 exact objects**. Ten
transaction tests, the expanded profile-family test, and 117 inline-expansion
tests pass; the previously documented embedded-asm composition failure
remains explicitly skipped.

The 40 real-project transport configurations retain **35 BYTE, one DIFF,
and four missing dependencies**, with **33/34 measured code-exact**, two
empty, and four unmeasured. Melee retains **16/21 exact functions** and
**1960/3320 exact reference function bytes**; its five remaining nonexact
functions are `CheckMailBox`, `DBInitComm`, `DBQueryData`, `DBRead`, and
`DBWrite`. This milestone extends the extracted transport behavior across
versions without claiming a new exact real-project configuration.

Local evidence: `target/stream-modern-*`, `target/check_stream_modern*.py`,
`target/probe_stream_modern.py`, and
`target/reference-parity/7e5436efe3182349-5e4ca1ddc460f4d8.jsonl`.

## Legacy fixed-bank word streams, 2026-09-06

Against baseline `a2dbd3c6`, Melee's `DBGRead` and `DBGWrite` now each match
all **220 reference function bytes**. The complete source improves from
**14/21 to 16/21 exact functions** and **1520/3320 to 1960/3320 exact reference
function bytes**, with all 21 compiling. Five functions remain nonexact:
`CheckMailBox`, `DBInitComm`, `DBQueryData`, `DBRead`, and `DBWrite`.

The existing fixed-bank transaction plan now includes a word-stream payload.
A semantic recognizer proves the selected-register update, command expression,
initial transfer, repeated word calls and polling, cursor increments, signed
remaining-byte decrement/clamp, reset, and boolean result. It derives the
bank, slots, masks, insertion bits, command geometry, direction, and callee;
function names and device addresses do not determine admission. Select,
call, poll, and reset emission are shared with the prior mailbox transactions.
Extra effects, altered loop bounds/steps/clamps, incompatible word storage,
aliased local roles, mismatched polls, and effectful apparent constants decline.

`FixedBankStreamStyle` resolves the measured frame and command policy by
profile. GC/1.1, GC/1.2.5, and GC/1.2.5n share 64-byte frames, fused command
rotate/mask operations, retained bank/slot homes, and the call/poll/clamp
schedule. GC/1.1p1 uses 56-byte frames, preserves source mask/shift order,
advances the write cursor earlier, and restores the stack before reloading LR;
its functions are 224 bytes. Other profile families retain structured lowering.
The stream owner requires O4 performance optimization and the measured
scheduling conditions; the general shift/mask fusion policy is unchanged.

New canaries **1604 and 1605 improve from 0/30 to 8/30 whole-object exact
pairs** across all 15 builds. Both objects match on all four legacy builds;
the other 22 pairs remain nonexact. All 30 pairs compile, with no exclusions
or reference rejections. The second canary changes the bank to `0xCC00F000`,
register indices, selection/poll masks, insertion bits, command shifts/masks
and tags, source typedefs, and public names.

**231,840 paired Unicorn cases pass**: 200,448 detailed caller cases across
the four changed legacy profiles and both new canaries, 6,336 smaller checks
across the other eleven profiles, and 25,056 executing Melee's actual read/write
caller bodies. An ABI-clobbering transfer model checks command words, data
payloads, buffer guards, device event order, stack/register restoration, and
return values. Cases include negative/zero lengths, partial final words,
multiple iterations through 32 bytes, varied busy waits, and failures at each
call position. The previous milestone's accumulator correctness is retained.

The 208-canary regression selection retains identical verdicts: **1,040 slots,
281 exclusions, 759 runnable pairs, 417 exact objects, and 223 existing
candidate rejections**, with no reference rejections or timeouts. An additional
120 neighboring build/canary pairs retain identical verdicts, including 57
exact objects. The fresh 40-configuration project selection remains **35 exact
objects, one DIFF, and four missing dependencies**; code remains exact on
**33/34 measured configurations**, with two empty and four unmeasured. Melee's
function coverage improves inside the remaining DIFF. Ten transaction admission
tests, one profile test, and 117 inline tests pass; the previously confirmed
embedded-asm composition failure remains excluded. Evidence is retained under
`target/stream-match-*`, `target/check_stream_match*.py`, and
`target/probe_word_streams.py`. These remain targeted diagnostics rather than
a corpus-wide parity estimate.

## Control-flow accumulators and initializer calls, 2026-09-06

Against baseline `8e32b501`, Melee's `DBGRead` and `DBGWrite` now retain their
error reductions across zero iterations and loop backedges. The baseline
writes each loop update to a new register but keeps reading the entry value
on subsequent iterations. It also returns the loop-only register when the
loop never runs. Paired execution reproduces both failures: a successful
zero-length operation incorrectly reports failure, and a middle transfer
failure is forgotten when a later transfer succeeds.

The existing named-value liveness graph is now `NamedValueFlow`, shared by
physical-home reservation and a reaching-definition check. Fresh assignment
homes are allowed only when each read sees the latest emitted definition on
all incoming paths. Otherwise boolean call-result assignments retain one
mutable home, including replacement assignments that do not OR the old value.
The check includes emission order so disjoint early-return arms cannot inherit
each other's definitions. Straight-line versioning remains available across
unrelated polling loops. Five new graph tests cover loops, joins, independent
return arms, unrelated polling, and unconditional redefinition after a join;
the seven previous physical-liveness tests remain intact.

A declaration-initializer variant exposed another ABI bug: eager saved-local
initializers could call a function before incoming parameters had been copied
to their saved homes. The subsequent copies captured clobbered argument/result
registers. Such initializers now select the existing batched save/copy path
and disable staggered copies. The check includes preceding declarations that
may supply initializer dependencies. Existing entry-alias retirement still
switches subsequent reads to the saved parameter homes after a call.

New canaries **1602 and 1603** cover accumulating and replacing loop updates,
conditional/alternative arms, early returns, continue/break, nested branches,
explicit gotos, call-containing initializers, and two incoming parameters.
Across all 15 builds, compilation improves from **8/30 to 30/30 pairs**; the
22 baseline rejections were excess saved-register demand from the incorrect
version splitting. Whole-object matching remains **0/30** for this selection,
with no build exclusions or reference rejections.

**89,268 paired Unicorn cases pass**: 26,520 for 1602, 30,300 for 1603, 25,056
for Melee's actual `DBGRead`/`DBGWrite` caller bodies, and 7,392 for the prior
straight-line accumulator canary. Caller tests intercept the transfer function
with an ABI-clobbering model, inject failures at different call positions,
and check command words, buffer payloads/guards, device event order, return
values, stack restoration, and saved registers. The reference and frozen
baseline reproduce the bugs before the fix. These checks do not claim new
instruction scheduling parity for the caller bodies.

The expanded 208-canary regression selection retains identical verdicts on
five builds: **1,040 slots, 281 exclusions, 759 runnable pairs, 417 exact
objects, and 223 existing candidate rejections**, with no reference rejections
or timeouts. The previous transfer selection remains **45/45 exact**. The
fresh 40-configuration project selection remains **35 exact objects, one
DIFF, and four missing dependencies**, with **33/34 measured code projections
exact**, two empty, and four unmeasured. Melee remains **14/21 exact functions
and 1520/3320 exact reference function bytes**. Twelve flow tests, eleven
entry-alias tests, six call-accumulator tests, and 117 inline tests pass; the
previously confirmed embedded-asm composition failure remains excluded.
Evidence is retained under `target/accumulator-flow-*`,
`target/accumulator-initializer-*`, and `target/check_accumulator*.py`.

## Mainline byte/word transfer schedules, 2026-09-06

Against baseline `314490c7`, canaries **1599–1601 improve from 12/45 to 45/45
whole-object exact pairs** across all 15 supported compiler builds. All pairs
compile, with no build exclusions or reference rejections. New canary 1601
combines inline saved-register frames (`-use_lmw_stmw on`) with 1600's three
source int/long combinations, alternate bank, register indices, and control
fields. These are focused transfer diagnostics, not corpus-wide parity rates.

The existing semantic recognizer now feeds separate mainline topology and
packet emitters. Eight 2.4.x builds share the packing/unpacking schedules,
32-byte frames, and inline scalar remainders. GC/3.0a3 and GC/3.0a3p1 add
signed-overflow eligibility checks and a revised unpack schedule; Wii/1.0
reschedules four unpack stores. Mainline preserves the source int/long
comparison distinction measured previously, while 4.x compares the bound
against zero in CR1 regardless of those equal-width source identities.

Normal flags use `_savegpr_26` / `_restgpr_26`; enabling multiple-register
loads/stores selects leaf frames with inline saves. Function sizes are 672
bytes on 2.4.x and 744 on 4.x with helper frames, or 648 / 720 with inline
saves. Wii's inline form is 724 bytes because its polling loop needs a padding
instruction. Alignment reuses the existing fixed-address polling profile
policy and derives padding from instruction position. Mainline admission
also checks that the folded control-register displacement fits a signed
immediate; otherwise it retains structured lowering.

**274,752 paired full Unicorn executions pass**: 38,880 for 1599, 116,640 each
for 1600 and 1601, and 2,592 for Melee's actual transfer body. They check
negative/zero counts, packet boundaries through 64 bytes, modes, data words,
busy-wait durations, randomized register/buffer contents, device event order,
return values, stack restoration, and saved registers. A further **2,205
paired bounded prefixes** stop at the first buffer access and verify both
its value and CTR. Counts around the unroll boundary and `INT_MAX` confirm
that 4.x takes the scalar path at `INT_MAX`, while earlier builds unroll.
This tests the huge-count branch without executing billions of iterations.

The 182-canary regression selection retains identical verdicts: **910 slots,
242 exclusions, 668 runnable pairs, 376 exact objects, and 183 existing
candidate rejections**, with no reference rejections or timeouts. The fresh
40-configuration project selection remains **35 whole-object exact, one
DIFF, and four missing dependencies**; code is exact on **33/34 measured**
configurations, with two empty and four unmeasured. Melee remains **14/21
exact functions and 1520/3320 exact reference function bytes**. Seven admission
tests, the profile-resolution test covering all 15 builds, and 117 inline
tests pass; the independently confirmed preexisting embedded-asm composition
failure remains excluded. Evidence is retained under `target/exi-modern-*`,
`target/check_exi_modern*.py`, and `target/probe_exi_modern*.py`.

## Legacy byte/word transfer unrolling, 2026-09-06

Against baseline `c6abb794`, Melee's `DBGEXIImm` now matches all **664 reference
function bytes**. The complete source improves from **13/21 to 14/21 exact
functions** and **856/3320 to 1520/3320 exact reference function bytes**, with
all 21 compiling. Seven functions remain nonexact: `DBGRead`, `DBGWrite`,
`CheckMailBox`, `DBInitComm`, `DBQueryData`, `DBRead`, and `DBWrite`.

A semantic recognizer proves the byte-pack loop, register write, control word,
busy wait, conditional register read, byte-unpack loop, and constant success
return. It derives the bank address, register offsets, control fields, poll bit,
and source variable roles. Separate packet emitters and the version-resolved
`ByteWordTransferStyle` own eight-byte unrolling, remainder loops, register
placement, and frame layout. GC/1.1, GC/1.2.5, and GC/1.2.5n share the 72-byte
frame and dependency-first packets. GC/1.1p1 uses a 64-byte frame, different
word/index/buffer homes, and interleaved packets. The legacy layout preserves
the reference's out-of-line packing-remainder setup after the result block.
Other version families retain structured lowering. Admission requires O4
performance optimization and the measured scheduling conditions.

Reference probes revealed that an `int` induction variable and `long` bound
retain register comparisons, while equal source types permit comparisons
against zero. Both types occupy 32 bits. The same distinction holds for a
`long` index with an `int` bound; making both `long` restores the equal-type
schedule. `SourceFunctionFacts` now carries the frontend's existing parameter
and local scalar identities alongside pointer memory facts into codegen.
Missing scalar identity facts do not authorize the new schedule. The compact
executable type representation remains unchanged.

New canary **1600** covers all three long-type combinations and changes the
bank to `0xCC00F000`, both register indices, the trigger/poll bit, and control
field shifts. Together with 1599, it improves **0/22 to 8/22 whole-object exact
pairs** across all 11 measured builds: both objects are exact on all four
legacy builds, and the other 14 pairs remain nonexact. All 22 pairs compile,
with no exclusions or reference rejections. Recognition rejects altered loop
bounds/steps, extra effects, signed byte or word storage, narrowing pointer
casts, aliasing local roles, mismatched device registers, and runtime
expressions that only appear constant through algebraic identities.

**116,640 paired Unicorn cases** pass: 28,512 for 1599, 85,536 for 1600's three
variants, and 2,592 executing Melee's actual transfer body. Checks retain the
previous milestone's transfer-length, shift-behavior, device-access, buffer,
return, and saved-register coverage. Ordinary input-byte loads may be
reordered within a contiguous read packet; device accesses and stores retain
execution order. Reference ABI save/restore helpers execute synthesized
load/store stubs where required by the other version families.

The 182-canary regression selection retains identical verdicts on five builds:
**910 slots, 242 exclusions, 668 runnable pairs, 376 exact objects, and 183
existing candidate rejections**, with no reference rejections or timeouts.
The 40-configuration reference selection retains **35 whole-object exact,
one DIFF, and four missing dependencies**. Code remains exact on **33/34
measured configurations**, with two empty and four unmeasured; Melee gains
function coverage within the remaining DIFF. Seven admission tests, one
profile-resolution test, and 117 inline tests pass, with the independently
confirmed preexisting embedded-asm composition failure excluded from the
inline selection. Evidence is retained under `target/exi-unroll-*.log`,
`target/check_exi_unroll*.py`, `target/probe_exi_unroll.py`, and the existing
immediate-transfer execution harness. These are focused diagnostics, not a
corpus-wide parity estimate.

## Physical register lifetimes in immediate transfers, 2026-09-06

Against baseline `8a795bb2`, Melee's `DBGEXIImm` now executes byte packing and
unpacking correctly in the measured cases. The baseline reused the incoming
data-pointer register for a packing shift count, causing an invalid memory
read on the second byte. Register-bank address materialization also overwrote
that pointer before the read-mode arm, causing a one-byte read to store into
the hardware bank instead of the caller's buffer. Both failures are reproduced
with the frozen baseline and pass against the reference after this change.

The fix addresses two shared mechanisms. Register allocation now retains the
complete union of each physical register's control-flow live slots; splitting
those slots at textual last uses had discarded loop-backedge and branch-join
lifetimes. Gaps between unrelated definitions remain reusable. Structured
statement selection separately computes named-value liveness over its emitted
label/if graph and reserves physical general-register homes across nested arms,
continuations, and loop backedges. This prevents an already selected scratch
instruction from destroying a value before allocation sees it. Retained
specialized loops and switches conservatively contribute their reads; owners
that synthesize separate statement trees retain their existing policies.

New canary **1599** reduces the real transfer routine, including its inlined
busy wait, byte-pack loop, control-register store, and byte-unpack loop. It
compiles on all 11 measured builds but remains **0/11 whole-object exact**.
The real GC/1.2.5 candidate shrinks from 184 to 180 bytes; the reference is 664
bytes and unrolls both loops. Melee remains **13/21 exact functions** and
**856/3320 exact reference function bytes**, with all 21 functions compiling.
This is a correctness milestone; it does not increase byte-exact coverage.

**64,368 paired Unicorn cases** pass: 28,512 transfer-canary cases across all
11 builds, 2,592 executing Melee's actual `DBGEXIImm` body, and 33,264 for the
existing mailbox/mask slice. Transfer checks cover read/write modes, three
register words, zero/one/four busy iterations, and four randomized register and
buffer seeds. Lengths include zero, negative values, 1–5, and unroll-boundary
values through 64. The longer lengths compare the reference's actual shift
behavior even where the C source does not define it. Checks compare device
access order/count, output bytes, input-byte reads, buffer guards, return values,
stack restoration, and saved general registers. Ordinary input-byte loads may
be reordered within a contiguous read packet. Reference save/restore helpers
execute synthesized ABI load/store stubs; the transfer body itself is executed.

The 182-canary regression selection has identical verdicts on five builds:
**910 slots, 242 exclusions, 668 runnable pairs, 376 exact objects, and 183
existing candidate rejections**, with no reference rejections or timeouts.
A separate 36-canary compatibility selection, covering 1570–1599 and six
older substring matches, retains **253/396 exact objects** across all 11
builds; every pair compiles, with no exclusions or reference rejections.
The 40-configuration reference selection retains **35 whole-object exact,
one DIFF, and four missing dependencies**; code is exact on **33/34 measured
configurations**, with two empty and four unmeasured.

Seven source-liveness tests and 99 register-allocation/scheduling tests pass
(the latter retain eight existing ignored tests). All 117 selected inline tests
pass, retaining the independently confirmed preexisting embedded-asm composition failure
as an explicit exclusion. Evidence is retained under `target/exi-imm-*.log`,
`target/check_exi_imm*.py`, and the existing mailbox execution harness.
These are focused diagnostic results, not a corpus-wide parity estimate.

## Legacy fixed-bank transfer transactions, 2026-09-06

Against baseline `7b70d0b9`, Melee's `DBGReadMailbox`, `DBGWriteMailbox`, and
`DBGReadStatus` now match reference function bytes and relocation targets.
The complete source improves from **10/21 to 13/21 exact functions** and
**372/3320 to 856/3320 exact reference function bytes**, with all 21 compiling.
Eight functions remain nonexact, including the underlying immediate-transfer
routine and the higher-level read/write callers.

A shared transaction recognizer describes the expanded select, payload
transfer, poll, optional read transfer and second poll, reset, and accumulated
status return. It derives the bank address, register offsets, field masks,
command/payload bits, and transfer symbol from the tree. A separate legacy
schedule retains the bank page and selected-slot address across calls; a
second poll retains its updated slot address as well. It preserves the
reference's distinct write/read frames and uses the existing linkage and
structured-frame policies for the patched and Nintendo distributions. The
schedule applies at O4 to the existing legacy profile family. Other profiles
continue through their existing shared lowering.

Admission verifies the full effect sequence, word storage, pointer arguments,
transfer prototype and return width, repeated poll condition, matching reset
mask, and status accumulator. Additional/volatile/static/aligned local storage,
changed transfer sizes or modes, pointer truncation, and intervening effects
are outside this schedule. Constants must be literal-derived expressions;
algebraic identities such as a difference of two register-bank reads do not
justify dropping those reads. No project function names or register-bank
addresses are built into recognition.

New canary 1598 changes the bank to a signed-low-half address, changes both
register indices, preserve/insert masks, poll bit, payload mask, and read/write
command bits. Canaries **1593, 1594, and 1598 match whole objects on all four
legacy builds**: GC/1.1, GC/1.1p1, GC/1.2.5, and GC/1.2.5n. Across all 11 measured
builds, exact objects improve **0/33 to 12/33**; all 33 pairs compile, with no
exclusions or reference rejections. The remaining 21 objects are nonexact.

**68,976 paired Unicorn cases** pass: 31,680 for the changed-bank variants,
33,264 for the existing mailbox/mask slice, and 4,032 for the three actual
Melee callers. Checks cover payload bytes and pointer outputs, zero/nonzero
transfer statuses, volatile access order/count, bank changes made by the
simulated device between calls, zero/one/four busy iterations, and stack and
saved-register restoration under four randomized clobber patterns. The Melee
checks simulate `DBGEXIImm` at the call boundary; they do not validate its body.

The 182-canary regression selection retains identical verdicts on five builds:
**910 slots, 242 exclusions, 668 runnable pairs, 376 exact objects, and 183
existing candidate rejections**, with no reference rejections or timeouts.
The 40-configuration reference selection retains **35 whole-object exact,
one DIFF, and four missing dependencies**. Code remains exact on **33/34
measured configurations**, with two empty and four unmeasured; Melee's function
coverage improves inside the remaining DIFF. Six transaction admission tests
and 117 backend inline tests pass, with the same independently confirmed
preexisting embedded-asm composition failure excluded from the inline selection.
Evidence is retained under `target/exi-address-*.log`,
`target/check_exi_address*.py`, and the prior mailbox execution harnesses.
These are focused diagnostic results, not a corpus-wide parity estimate.

## Accumulator lifetimes and empty-poll entries, 2026-09-06

Against baseline `bdd1199b`, structured lowering folds the first
`error |= !operation()` into an assignment when a zero declaration initializer
reaches it unchanged. The straight-line prefix may contain unrelated calls and
stores, but earlier reads, writes, address escapes, initializer aliases, labels,
control flow, volatile/static storage, and embedded assembly prevent the fold.
The operation's arguments must not refer to the accumulator. This removes an
unnecessary entry value, OR, and cross-call lifetime without moving the call.
The new fold is disabled at O0. If the resulting value survives a later call,
frame planning still reserves a saved home for it.

This exposed a separate frame-reconciliation defect: equal planned and required
save counts could hide different physical registers. A logical home could
color into caller-saved r4 while a result used r30 across a call, leaving r30
unsaved. Reconciliation now checks physical-register coverage before retaining
the existing saves, and uses the existing canonical save reconstruction when
the sets differ. Runtime checks verify saved-register preservation after the
new lifetime transformation.

Empty structured loops with no step now reach their first condition by
fallthrough. This removes a branch to the next instruction in composed polls
while retaining every condition evaluation. Empty for-loops with a step keep
their entry test ahead of that step; the existing assembly-sensitive for-loop
entry policy also remains in effect.

New canaries 1596--1597 cover call accumulators and composed waits for both a
clear and a set bit. Across **1593, 1594, 1596, and 1597 on 11 builds**, all
**44 pairs compile**, with no exclusions or reference rejections; whole-object
exactness improves **0/44 to 9/44**. The polling object matches on every measured
build except GC/1.1 and GC/1.1p1. Accumulator and mailbox objects remain nonexact.
The actual Melee caller sizes improve from **216/160/216 to 200/140/200 bytes**
for read-mailbox/write-mailbox/read-status; reference sizes are 172/140/172.
The write length now agrees, but its schedule and register allocation do not.
Melee retains **10/21 exact functions and 372/3320 exact reference bytes**.

**101,184 paired Unicorn execution cases** pass: 7,392 for the new accumulator
canary, 1,320 for the new polls, 33,264 for the prior mailbox/mask slice, 55,176
for earlier EXI composition, and 4,032 for the three actual Melee callers.
Checks cover call arguments/order, varied transfer and operation statuses,
volatile read/write counts and values, zero/one/four waiting iterations, stack
restoration, and saved registers under four randomized clobber patterns. The
Melee checks simulate `DBGEXIImm` at the call boundary and do not validate its
implementation. Earlier EXI objects retain **53/88 exact**, with all 88 compiling.

The expanded 182-canary regression selection retains identical verdicts on five
builds: **910 slots, 242 exclusions, 668 runnable pairs, 376 exact objects, and
183 existing candidate rejections**, with no reference rejections or timeouts.
The 40-configuration reference slice is unchanged: **35 whole-object exact,
one DIFF, four missing dependencies**; **33/34 measured code comparisons exact**,
with two empty and four unmeasured. Six accumulator tests, nine structured-loop
tests, and 117 backend inline tests pass; the inline selection continues to
exclude the independently confirmed preexisting embedded-asm composition
failure. Evidence is retained in `target/exi-accumulator-*.log`,
`target/check_exi_accumulator*.py`, and the prior mailbox/EXI execution harnesses.
These targeted diagnostics do not establish corpus-wide parity.

## General word masks and expanded EXI mailbox callers, 2026-09-06

Against baseline `403da4ec`, the shared integer expression selector now handles
noncontiguous positive word masks. A mask occupying one halfword selects
`andi.` or the newly represented `andis.`; a mask spanning both halves is
materialized for a register AND. The high-half instruction participates in the
normal GPR use/definition traversal. Constant materialization uses wrapping
carry adjustment so values such as `0x7FFF8005` do not overflow the host's
signed arithmetic. Existing rotate-mask and negative-immediate choices remain
in place; the new full-word materialized path admits word register operands.

The terminal store/return precheck now recognizes fixed-address banks from
their own addressing table. Previously it classified them as ordinary pointer
stores and rejected expanded mailbox callers before structured lowering. With
that corrected and the mask expression supported, Melee's `DBGReadMailbox`,
`DBGWriteMailbox`, and `DBGReadStatus` compose select, poll, and deselect through
the shared structured backend. Only their two/one/two `DBGEXIImm` calls remain,
matching the reference's call structure. The instruction schedules and frame
layouts remain nonexact: candidate sizes are 216/160/216 bytes versus baseline
196/156/196 bytes. The complete source still compiles all 21 functions and
retains **10/21 exact functions, 372/3320 exact reference function bytes**.

New canaries 1593--1595 cover write/read transactions and six mask forms,
including volatile input and high-half carry boundaries. Across **33 runnable
pairs on 11 builds**, exact objects improve **0/33 to 11/33** and candidate
compilation improves **22/33 to 33/33**. There are no exclusions or reference
rejections. All builds match the mask object; both transaction objects remain
nonexact. **33,264 paired Unicorn cases** verify the new slice with varied
inputs, transfer statuses, zero/one/four busy iterations, and four randomized
register/clobber patterns. The transfer hook changes the bank between accesses
to verify the final volatile reload, checks payload bytes and arguments, and
writes read results through the supplied pointer. Stack and saved-register
restoration are checked. The three actual Melee caller functions also pass
**4,032 paired cases** against reference objects with `DBGEXIImm` simulated at
the call boundary; this does not validate the transfer implementation itself.

Earlier EXI canaries 1585--1592 retain **53/88 exact objects**, all 88 compile,
and all **55,176 paired execution cases** pass. The 99-canary mask, fixed-bank,
inline, and status regression selection retains identical verdicts on five
builds: **495 slots, 152 exclusions, 343 runnable pairs, 253 exact objects,
and 48 existing candidate rejections**, with no reference rejections or
timeouts. The 40-configuration real-project selection is unchanged: **35
whole-object exact, one DIFF, four missing dependencies**; code is exact on
**33/34 measured configurations**, with two empty and four unmeasured.

Validation: three store-hazard tests, the existing negative-mask test, 117
backend inline tests, nine machine-code tests, and 97 register-allocation and
schedule tests pass (eight preexisting ignored tests). The inline selection
continues to exclude the independently confirmed preexisting embedded-asm
composition failure. Evidence is retained under `target/exi-mailbox-*.log`,
`target/check_exi_mailbox*.py`, and the existing EXI guard harnesses. These are
focused diagnostic slices, not an estimate of corpus-wide parity.

## Ordered status guards and constant-channel EXI selects, 2026-09-06

Against baseline `76059e78`, the zero-status canary 1590 now matches whole
objects on **all 11 measured builds**. The composer turns trailing guards into
an ordered statement exit chain when exposing a helper in a guard condition,
guard value, or final return. Earlier exits stay ahead of later effects, guard
values execute only on their taken edge, and an unconditional selected return
removes unreachable following effects. The legacy boolean-call owner now gives
optional inline composition first refusal and retains its previous linked-call
schedule as a fallback. The direct word-register clear owner can also preserve
an incoming word result, including its distinct legacy address schedule.

New canary 1592's constant-channel select, returned status, select/clear pair,
and select followed by a forwarded call match whole objects on **all 11
builds**. Constant and parameterized fields share the existing word-field
recognizer. A focused constant-field scheduler uses the measured legacy versus
later address/status placement, early-generation reset mask home, and compact
linked-call or modern sibling-call continuation. Call admission verifies the
word arguments' ABI homes and excludes indirect, variadic, intrinsic, and
embedded-assembly targets. Wider early-generation reset/passthrough mask homes
remain outside these measured schedules.

Canaries 1591--1592 add guard-order and constant-channel coverage. Across
1585--1592 there are **88 oracle-runnable pairs**, with no exclusions or
reference/candidate rejections. Exact objects improve **31/88 to 53/88**.
The guard-order object remains nonexact, while execution checks verify its
prefix call, early-exit behavior, conditional poll result, and suppression of
later effects. **55,176 paired Unicorn execution cases** pass across reference
and candidate, checking volatile access order/count, masked values, status and
passthrough returns, argument forwarding, zero/one/four waiting iterations,
stack restoration, and saved registers under four randomized register/clobber
patterns.

Both focused regression selections retain identical verdicts on five builds.
The 63-canary inline/status/fixed-register selection has **315 slots, 120
exclusions, 195 runnable pairs, 125 exact objects, and 30 existing candidate
rejections**. The 81-canary guard/boolean selection has **405 slots, 110
exclusions, 295 runnable pairs, 119 exact objects, and 130 existing candidate
rejections**. Neither has reference rejections or timeouts; the selections
overlap and are reported separately.

The 40-configuration real-project stub family retains **35 BYTE, one DIFF,
zero DEFER, and four MISSING_DEPENDENCY**, with **33/34 measured code exact**,
two empty objects, and four unmeasured rows. Melee's configured GC/1.2.5
`odenotstub.c` still compiles all 21 functions with **10/21 exact functions and
372/3320 exact reference function bytes**. The isolated channel-four operation
is now verified; its larger EXI callers and transfer loops remain nonexact.

Compiler/oracle builds, 31 frontend inline tests, and 117 backend inline tests
pass. The backend run explicitly skips the previously confirmed existing
embedded-assembly composition failure. Guard-order tests now check the
normalized final-return representation; new tests cover guarded result effects
and unreachable effects after a known taken guard. Local reproduction artifacts
use `target/check_exi_guard_*.py`, `target/exi-guard-*.log`, and the fingerprinted
reference-parity cache.

## Combined EXI status transactions, 2026-09-06

Against baseline `fc949660`, canary 1585's complete select/deselect transaction
now matches whole objects on **all 11 measured builds**. Constant-result helper
calls in statement conditions retain their argument evaluation and memory
effects at the original condition position, then select the known branch.
A status accumulator that becomes immutable can be removed using the existing
escape/modification proof; volatile, static, escaped, and reassigned storage
is retained. This exposes the straight-line register transaction to its
existing lowering owner instead of embedding stores inside a condition value.

The parameterized RMW owner can now append a compound clear using the same
bank slot and mask. It shares the address and, where applicable, mask register,
while retaining both volatile reads and writes. Legacy combined transactions
use the discarded-status allocation with the final status placed before the
first store. Later builds extend their existing status-return schedules.
Early-generation wide masks with a reset remain outside this owner pending
measurement of the mask's surviving register home.

New canaries 1589--1590 cover non-one success statuses, true/false branches,
side-effecting arguments, zero statuses, and an incoming accumulator. Together
with 1585--1588, this is **66 oracle-runnable pairs**, with no exclusions or
reference/candidate rejections. Whole-object matches improve **20/66 to 31/66**;
the two new objects remain nonexact. The new `branch_true` function matches
all 11 builds; `branch_false` matches seven. The zero-status canary records the
remaining trailing-guard and register-passthrough lowering gaps. **35,376 paired
Unicorn execution cases** pass, including one evaluation of `next_channel`,
the selected branch's effects, volatile access order/count, masked values,
returned statuses, preserved incoming words, stack restoration, and saved
registers under four randomized register/clobber patterns.

The 63-canary inline/status/fixed-register regression slice retains identical
verdicts in **315 slots: 120 exclusions, 195 runnable pairs, 125 exact objects,
and 30 existing candidate rejections**, with no reference rejections or
timeouts. The real-project stub family remains **35 BYTE, one DIFF, zero DEFER,
and four MISSING_DEPENDENCY** across 40 configurations, with **33/34 measured
code exact**, two empty objects, and four unmeasured rows. Melee's configured
GC/1.2.5 `odenotstub.c` still compiles all 21 functions and retains **10/21 exact
functions, 372/3320 exact reference function bytes**, and its previous direct
call targets. This is a verified transaction building block; broader constant-
channel EXI callers and their transfer-loop schedules remain incomplete.

Compiler/oracle builds, 31 frontend inline tests, and 115 backend inline tests
pass. The backend run explicitly skips the previously confirmed existing
embedded-assembly composition failure. Three new tests check branch placement,
argument effects, zero-result effects, and accumulator cleanup boundaries.
Local reproduction artifacts use `target/check_exi_transaction_*.py`,
`target/exi-transaction-*.log`, and the fingerprinted reference-parity cache.

## Automatic EXI status-helper inlining, 2026-09-06

Against baseline `ba745c4f`, the automatic inline composer admits small
constant-result register-bank updates and empty hardware polling loops.
A focused summary module describes their effects and known status. Statement
composition retains argument materialization, fresh local names, source
visibility, and recursion accounting; straight-line updates can also use the
existing sequenced value summary. Exposing a known result preserves the call's
evaluation point, leaves short-circuit expressions and trailing guards in
place, and rejects capture by caller-bound bank names. Only freshly introduced
entry locals can regain their declaration initializer.

The existing poll owner now preserves a returned entry word or forwards word
arguments to a following direct call. It uses the measured compact linkage
frames on older builds and sibling transfers on modern builds, keeping address
setup outside the volatile loop and live arguments out of its scratch pool.
Full-word masks fold to the truthy poll without overflowing the mask check.
The parameterized fixed-register RMW owner also handles discarded status,
including the early-generation wide-mask schedule, using its existing version
policy. A boundary canary exposed a preexisting scope bug, reproduced with the
baseline binary: fixed-address metadata overrode a same-named pointer parameter.
Per-function address maps now exclude names bound by parameters and locals.

Canaries 1585--1588 provide **44 oracle-runnable pairs across 11 builds**, with
no exclusions or reference rejections. Candidate compilation improves
**33/44 to 44/44** and whole-object matches improve **0/44 to 20/44**. The
poll/status canary matches all 11 builds; the wide-mask/forwarding canary
matches nine. The fixed-update canary's first four functions match, while its
combined select/deselect transaction still retains calls. Guarded, repeated,
and shadowed-name polling shapes also remain nonexact. **23,496 paired Unicorn
execution cases** pass against reference and candidate, checking volatile
access order and count, zero/one/four waiting iterations, masked writes, status
and passthrough returns, one/two forwarded arguments, short-circuit and guarded
exits, shadowed pointer storage, stack restoration, and saved registers under
four randomized register/clobber patterns.

The 63-canary inline/status/fixed-register regression slice on five builds has
**315 slots, 120 declared exclusions, and 195 runnable pairs**. Both baseline
and candidate have **125/195 exact objects**, with no changed verdicts, no
reference rejections or timeouts, and 30 existing candidate rejections. The
40-configuration real-project stub family remains **35 BYTE, one DIFF, zero
DEFER, and four MISSING_DEPENDENCY**, with **33/34 measured code exact**, two
empty objects, and four unmeasured rows.

Melee's configured GC/1.2.5 `odenotstub.c` still compiles completely and retains
**10/21 exact functions and 372/3320 exact reference function bytes**, with no
missing or candidate-only functions. `DBGEXIImm` now contains the `DBGEXISync`
poll instead of calling it; its other loop and scheduling differences remain.
This milestone establishes helper coverage and execution checks, not additional
exact Melee functions or complete-project parity.

Compiler/oracle builds, 31 frontend inline tests, and 112 backend inline tests
pass. The unfiltered backend run still reports the previously confirmed
embedded-assembly composition failure documented below; the verified run
explicitly skips that one test. Six new unit tests cover evaluation placement,
short-circuit/guard boundaries, name capture, scalar status conversion, and
initializer ownership. Local reproduction artifacts are
`target/check_exi_inline_{baseline,candidate,regression,semantics}.py`,
`target/exi-inline-*.log`, and the fingerprinted reference-parity cache.

## Callback registration schedules and narrow argument constants, 2026-09-06

Against baseline `70eac5d5`, Melee's configured GC/1.2.5 `odenotstub.c` gains an
exact `DBInitInterrupts`, including its relocations. The complete object now
has **10/21 exact functions, up from 9/21**, and **372/3320 exact reference
function bytes, up from 288/3320**. All 21 functions remain comparable, with
none missing or candidate-only. The full-source verdict remains **DIFF**;
eleven functions still differ.

The existing callback argument scheduler now borrows the unfinished first
integer argument's register for a second-argument function address on legacy
linkage-first builds. Publication scheduling overlaps an independent handler
address with the preceding callback store, preserving the distinct entry and
post-call orders, compact legacy exit, and versioned symbol discovery rules.
Middle-generation builds retain data-first symbol grouping. The scheduler
validates both address relocation pairs and keeps the transformation within a
linear machine body, including control flow introduced by retained inlines.

Conversion probes exposed an existing ABI error: integer constants passed to
narrow parameters bypassed their declared conversion (`signed char` received
255 instead of -1, and `unsigned char` received -1 instead of 255). The common
argument entry now converts out-of-range constants before selecting a schedule,
using the same integer conversion helper as explicit casts. Unchanged values
retain their original expression and scheduling path. A new integration test
checks the reference's converted argument constants on three compiler builds.

New canaries 1582--1584 provide **33 oracle-runnable pairs across 11 builds**,
with no exclusions, reference rejections, or candidate rejections. Whole-object
matches improve **5/33 to 20/33**: callback publication/registration matches all
11 builds, and constant callback arguments match nine. The two modern terminal
wrappers still differ in sibling-call selection; the broader conversion canary
still has scheduling differences. **1,760 paired Unicorn execution cases**
(reference and candidate) pass, checking the published address and its order
relative to calls, signed/unsigned byte and halfword conversions, large words,
forwarded and loaded arguments, stack restoration, and saved registers under
eight randomized caller-clobber patterns.

Fourteen older narrow-argument/store canaries now use ASCII punctuation in
comments so both compilers can consume the same regression sources. With those
identical cleaned sources, the 60-canary callback/argument/narrowing slice on
five builds has **300 slots, 87 declared exclusions, three existing reference
rejections, and 210 runnable pairs**. Exact objects improve **135/210 to
141/210**, with no lost matches. Eight existing candidate rejections remain;
there are no encoding failures or timeouts. Comment cleanup is not counted as
a compiler parity gain.

The 40-configuration real-project stub family retains **35 BYTE, one DIFF,
zero DEFER, and four MISSING_DEPENDENCY**, with **33/34 measured code exact**,
two empty objects, and four unmeasured rows. Compiler/oracle builds, 45 callback
backend tests, 30 argument integration tests, and the explicit integer-cast
test pass. The backend argument filter passes 124 tests; its one other test is
the previously confirmed existing embedded-assembly composition failure
recorded below, and the repeat run explicitly skips that test. These targeted
checks do not establish whole-project parity.

## Complete EXI source compilation and typed callbacks, 2026-09-06

Melee's complete `extern/dolphin/src/dolphin/odenotstub/odenotstub.c` now compiles
with its configured GC/1.2.5 project flags. Against baseline `9fae6f72`, the
configured verdict changes from **DEFER to DIFF** and the comparison advances
from 18 comparable functions in a partial object to **all 21 functions in the
complete object**, with none missing or candidate-only. `MWCallback` and
`DBGHandler` now match code and relocations, improving **7/21 to 9/21 exact
functions** and **164/3320 to 288/3320 exact reference function bytes**. This
does not establish matching output or execution correctness for the complete
transport: its other twelve functions remain nonexact.

Scalar global callback lowering now owns the null test, optional constant
prefix store, and forwarded arguments as one region. It loads the callback
once and retains the versioned linkage-register/CTR call convention, prefix
store order, symbol creation order, and return sequence. Modern profiles use
an unlinked sibling call. The previous zero-argument matcher and its tests
move into the same focused module; indexed callback-table lowering remains
separate. Admission excludes volatile callback pointers, intervening effects,
variadic signatures, and argument conversions this owner cannot preserve.

The final `DBWrite` rejection was a computed condition selecting a constant
against zero. When existing mask owners decline, this expression now retains
its source branch diamond instead of requiring constants to have register
homes. File-scope function-pointer parameter and variadic metadata also reach
the existing call marshaler, so integer literals passed to floating parameters
use FPRs and the correct pool width. Linked and sibling global calls share
argument preparation; floating constant loads retain the measured frame-era
ordering. Variadic floating arguments receive double promotion, and the EABI
CR marker reflects the declared argument classes as well as the variadic tail.

New canaries 1577--1581 have **55 oracle-runnable pairs across 11 builds**, with
no exclusions or reference rejections. Candidate compilation improves
**22/55 to 55/55**, and whole-object matches improve **0/55 to 33/55**. The two
guarded callback canaries and the floating-parameter callback canary match all
11 builds. Loaded-condition selects and variadic callback schedules remain
nonexact. **6,259 paired Unicorn execution cases** pass, checking null and
nonnull callbacks, prefix memory transactions, signed arguments, all 256 byte
conditions, floating values (including double `1.1`), variadic CR markers,
call order, stack restoration, and saved registers under caller clobbers.

Eighteen older canaries had unencodable punctuation in comments; those comments
now use ASCII. Baseline and candidate use identical cleaned sources. The
40-canary callback/select regression slice on five builds has **200 slots,
27 exclusions, and 173 runnable pairs**: **104/173 to 119/173 exact**, with no
lost matches and candidate rejections reduced from 24 to nine. The separate
ten-canary variadic slice has **50 slots, 30 exclusions, and 20 runnable pairs**,
retaining **11/20 exact** with no candidate rejections. Neither slice has
reference rejections, encoding failures, or timeouts.

The real-project 40-configuration stub family retains **35 BYTE** and now has
**one DIFF, zero DEFER, four MISSING_DEPENDENCY**; its executable projection
remains 33/34 measured exact, with two empty objects and four unmeasured rows.
Compiler/oracle builds, 45 callback backend tests, 28 select backend tests, and
29 argument integration tests pass. A broader `call` unit-test filter reports
959 passes and one existing failure,
`inline_expansion::tests::composes_zero_argument_embedded_asm_at_a_nested_call_site`;
the same isolated failure is reproduced in a detached `9fae6f72` checkout.
Full-project parity remains open.

## Computed EXI stores and initialized call accumulators, 2026-09-06

Against baseline `a498a9ad`, Melee's configured `odenotstub.c` partial object
now contains **18/21 comparable functions, up from 13/21**, with three missing
and no candidate-only functions. Exactness remains **7/21 functions and
164/3320 reference function bytes**, including relocations. `DBGEXIImm` and
four more transport helpers now lower. The complete source still **DEFERs**,
now at `MWCallback`'s guarded global function pointer. This is compilation
coverage progress, not an increase in full-source or partial-function matches.

Constant-index fixed-address integer stores now accept computed values through
the existing value emitter. Their address register is allocated after value
evaluation, so calls and temporary registers cannot clobber a live bank base.
Existing variable and constant schedules retain their version policies.
Structured call accumulators with declaration initializers retain an entry
home, including nonzero initial values and values used after a guarded call.

Execution probes also exposed an unsafe leading-guard transformation: an
assignment containing `!call()` could move before an early return on newer
builds. The transformation now checks assignment values and guard expressions
for side effects, preserving the observable call order. An integration test
checks the reference entry sequence that saves the initial accumulator and
calls the guard before updating it.

New canaries 1574--1576 cover computed control words, arithmetic and loaded
values, volatile bank reads, initialized boolean call chains, and call results
stored at word, halfword, and byte widths. All **33 pairs across 11 builds**
are oracle-runnable: candidate compilation improves **0/33 to 33/33**, while
whole-object exactness improves **0/33 to 11/33**. The call-result store canary
matches every build; arithmetic/register reuse and accumulator schedules remain
nonexact. A focused Unicorn execution experiment passes **2,673 paired cases**
(each executed in both reference and candidate), checking return values,
observable hardware reads/writes, call order and arguments, narrowing, stack
restoration, and saved registers while clobbering caller-saved registers at
external calls. These are probe results, not whole-project execution coverage.

The broader fixed-register/indexed-update/accumulator selection has **33
canaries on five builds, 165 slots, 46 declared exclusions, and 119 runnable
pairs**: **81/119 to 86/119 whole-object exact**, with no lost matches and
candidate rejections reduced from 15 to zero. A separate 15-canary guard slice
on four builds retains **21/31 exact**, with 29 exclusions among 60 slots and
four existing candidate rejections. Neither slice has reference rejections or
timeouts. The 40-configuration real-project stub family retains **35 BYTE,
zero DIFF, one DEFER, four MISSING_DEPENDENCY**, and 33/34 measured code matches.
Compiler/oracle builds pass, as do the focused test filters: 34 fixed-register,
163 guard, two accumulator backend tests, and the new integration test.

## EXI register primitives and signed status returns, 2026-09-06

Melee's `extern/dolphin/src/dolphin/odenotstub/odenotstub.c` contains a complete
EXI debugger transport. Against baseline `280ba0d6`, its configured-source
partial-TU comparison improves from **3/21 to 7/21 exact functions**, including
relocations, and **16/3320 to 164/3320 exact reference function bytes**. The four
gains are `DBGEXIInit`, `DBGEXISelect`, `DBGEXIDeselect`, and `DBGEXISync`.
Comparable functions increase from ten to 13; eight remain missing, with no
candidate-only functions. The full source still **DEFERs** in `DBGEXIImm`, whose
computed fixed-register store is not yet supported. Partial objects are not
counted as whole-object parity.

Existing fixed-register owners now accept signed `BOOL` constant returns as
well as unsigned status returns. Compound-update provenance is exposed to the
fixed-slot mask matcher while remaining available for scheduling: 2.3.3 loads
before completing the store base for explicit assignment and after it for
compound assignment; build 53 uses a separate mask register for compound form.
Masks above `0x7fff` require a correctly zero-extended two-instruction constant
on build 53, including its distinct register reuse in parameterized updates.

A direct constant-slot store now folds the bank low address into the store's
displacement. The 2.3.3 full-bank-address policy remains with reads and the
existing bank-reuse/RMW owners. Polling functions can return a constant status
after the volatile loop; the 4.x profile selects a folded load displacement,
and Wii additionally aligns that load to eight bytes. These refinements reuse
the existing instruction generators and explicit version policies.

New canaries 1570--1573 cover signed-status selection, compound versus explicit
masking, direct/parameter/post-call stores, and polling with and without a
status result. Both mask canaries also exercise `0x8001`. They improve from
**7/44 to 44/44 whole-object exact** across 11 builds. Existing canaries
1263--1265 now include their previously excluded patch/modern builds; the
combined seven-canary check improves from **25/77 to 77/77 exact**, with all
pairs oracle-runnable and no exclusions or reference rejections. Baseline and
candidate use the same expanded sources and build directives.

The broader fixed-register/indexed-update selection contains **27 authored
canaries on five builds** (GC/1.2.5, GC/1.3, GC/1.3.2, GC/3.0a3, Wii/1.0):
**46/102 to 80/102 whole-object exact**, with no lost matches. Its 135 slots
include 33 declared exclusions, no reference rejections, and no timeouts.
The full 40-configuration `odenotstub.c` family retains **35 BYTE, zero DIFF,
one DEFER, four MISSING_DEPENDENCY**. Its executable projection remains
33/34 measured exact, with two empty objects and four unmeasured configurations.
Compiler/oracle builds and **74 targeted unit tests** pass: 34 fixed-register
backend tests and 40 version-profile tests. Full-project parity remains open.

## Character-mode object metadata, 2026-09-06

All seven remaining nonexact modern SDK stub objects now match byte for byte.
Against baseline `d0f68372`, the complete 40-configuration `odenotstub.c` family
improves from **28 BYTE, seven DIFF, one DEFER, four MISSING_DEPENDENCY** to
**35 BYTE, zero DIFF, one DEFER, four MISSING_DEPENDENCY**, with no lost matches.
The gains are Twilight Princess GC/3.0a3p1 RZDE01_00, RZDE01_02, RZDJ01,
RZDP01, DZDE01, plus Wii/1.0 Shield and ShieldD. Executable code and relocations
are unchanged: **33/34 measured exact**, two empty objects, four unmeasured
configurations. Melee's remaining configured-source failure reaches unsupported
value tracking in `DBGEXISelect`; it remains in the denominator.

Each newly matching object differed only at byte 22 of the Metrowerks `.comment`
header. The 4.x object format records resolved unsigned plain-character mode
there, even in functions that never use a character type. Older formats keep
that byte zero regardless of `-char`. The driver now passes the resolved mode
to the existing comment-format descriptor, and the object writer applies the
format-generation rule. Signed/default mode and older format bytes are
preserved. Direct probes cover default, signed, and unsigned modes across nine
reference builds, including the patched GC/3.0a3p1 executable.

New canary 1569 isolates this metadata with an integer-only function and
`-char unsigned`: **9/11 to 11/11 whole-object exact**. Existing canary 1132
checks unsigned character loads, promotions, and compound stores and also
improves from **9/11 to 11/11**. All **22/22 pairs** are oracle-runnable with
no exclusions or reference rejections.

Nine older character canaries contained em dashes in comments that prevented
our compiler from accepting them through the harness's Shift-JIS path. Those
comments now use ASCII punctuation, preserving code and line counts. Both
baseline and candidate measurements use the corrected sources. The broader
character selection has **22 authored canaries on three builds** (GC/2.7,
GC/3.0a3, Wii/1.0): **19/36 to 23/36 whole-object exact**, with no lost matches,
30 declared exclusions, and no encoding failures, reference rejections, or
timeouts. Compiler/oracle builds and **31 object-writer tests** pass, including
the cross-format character-mode test. These targeted measurements do not
establish whole-project or corpus-wide parity.

## GC/2.7 fragmented debug and SDK stub metadata, 2026-09-06

Twilight Princess **GZ2E01, GZ2J01, and GZ2P01** `src/odenotstub/odenotstub.c`
now match byte for byte on GC/2.7. Against baseline `6eb6dfb1`, the complete
40-configuration source family improves from **25 BYTE, ten DIFF, one DEFER,
four MISSING_DEPENDENCY** to **28 BYTE, seven DIFF, one DEFER, four
MISSING_DEPENDENCY**, with no lost matches. Executable bytes and relocations
remain **33/34 measured exact**, with two empty objects and four unmeasured
configurations. These counts include every configured row, including failures.

GC/2.7's internal version is still 2.4.7, but build 108 already uses named
`.line.*` and `.dwarf.*` fragments. `CompilerBuild::debug_format` now selects
monolithic, fragmented 2.4.7, or fragmented 4.x policy independently of optimizer
version. Shared fragment construction retains GC/2.7's earlier section order,
local-function/header publication order, and four-ordinal assembly scope
discount. Two consecutive assembly bodies consume no ordinary function scopes;
a following C body resumes the ordinary timeline. Debug source analysis reuses
the frontend's measured declaration weights: GC/2.7 charges const locals but
neither mutable locals nor parameter names.

The SDK stub also exposed two independent metadata rules. `cats off` suppresses
unreferenced C inline-assembly symbols, whose retention otherwise supports code
address tables; referenced symbols still retain their ordinary bindings. A
literal narrow-integer return uses the same line seam as an ordinary integer
constant return. The general mixed-function debug plan now models that seam,
including the distinct 2.3.3, 2.4.x, and 4.x line conventions.

New canaries 1566--1568 isolate mixed assembly/C scope numbering, automatic
local declaration costs, and the SDK's disabled-catalog inline helpers plus
narrow return. Their **33/33 pairs run across 11 builds**, improving from
**0/33 to 14/33 whole-object exact** with no exclusions or reference rejections.
Canaries 1566 and 1568 each become exact on GC/1.3, GC/1.3.2, GC/1.3.2r,
GC/2.6, GC/2.7, GC/3.0a3, and Wii/1.0. Canary 1567 retains debug differences.

The paired debug/inline regression selection contains **77 authored canaries on
four compilers** (GC/2.6, GC/2.7, GC/3.0a3, Wii/1.0): **67/161 to 89/161
whole-object exact**, with no lost matches. All 308 slots are accounted for:
147 declared exclusions, no reference rejections, and no timeouts. The existing
74-canary subset gains 14 matches, including 11 on GC/2.7. All **54 OSSync**
configurations were rechecked: **51 BYTE and three reference-side HARNESS
failures**, preserving **51/51 measurable whole objects and code projections**.
Compiler/oracle builds and **144 targeted unit tests** pass (30 object,
40 version-profile, 74 debug-info).

An exploratory selection of the 35 smallest available GC/2.7 debug sources in
Twilight Princess GZ2E01 improved from one to two exact objects; its remaining
three DIFF, 14 DEFER, and 16 HARNESS rows expose substantial outstanding work.
That diagnostic is not a project-parity estimate, and neither the stub family
nor the canary checks establish whole-project or corpus-wide parity.

## Wii entry alignment and unoptimized vector copies, 2026-09-06

Both remaining Wii OSSync objects now match byte for byte: Twilight Princess
**Shield and ShieldD**. Against baseline `688a7369`, all 54 inventory
configurations ending in `/OSSync.c` improve from **49 BYTE, two DIFF, three
HARNESS** to **51 BYTE, zero DIFF/DEFER, three HARNESS**. All **51/51 measurable
objects and executable sections with relocations are exact**, with no empty
objects and no lost matches. The three remaining GC/3.0a3 GZ2E01/GZ2J01/GZ2P01
configurations are rejected by the reference compiler on an undefined `u32`
type; they remain in the configured denominator.

Assembly `entry` labels now record four-byte instruction alignment independently
of the containing function's alignment. The optimized Wii object differed only
in these two metadata records. Both local- and global-function entry-symbol
emission paths use that instruction-boundary rule.

The existing vector-copy owner now models unoptimized initializer-call bodies
under the predecrement frame convention. The symbol-range calculation preserves
both reads of the start address and reuses the range argument's register as
scratch. A version-profile policy selects Wii's additional reuse for the first
source address; the GC builds form that address directly in its argument
register. The destination's actual register home feeds ordinary debug-variable
provenance, and line records retain the initializer-result move on the local's
source line.

New canary 1564 isolates assembly entry-label metadata and improves from
**9/11 to 10/11 exact**. New canary 1565 reproduces the unoptimized mapped
vector and improves from **0/11 to 6/11 exact**: GC/1.3, GC/1.3.2,
GC/1.3.2r, GC/2.6, GC/3.0a3, and Wii/1.0. All 22 build/canary pairs are
oracle-runnable, with no exclusions or reference rejections. GC/2.7 retains
debug-container differences; the four older builds retain code differences.

The oracle now recognizes `-opt` as part of the `-O` option family. Previously,
its default `-O4,p` followed an explicit `-opt off`, so the reference ran at the
wrong optimization level. Both sides of the reported baseline/candidate
comparison use the corrected harness, with the baseline compiler binary
preserved. A unit test verifies default optimization removal for that alias.

The broader paired debug/inline selection contains **74 authored canaries on
two modern compilers**: **24/72 to 28/72 whole-object exact**, with no lost
matches. Its 148 slots include 76 declared exclusions, zero reference
rejections, and zero timeouts. Besides the new canaries, the existing SDK
command-line-options canary 1560 becomes exact on Wii. Compiler/oracle builds
pass, as do **148 targeted unit tests**: 30 object, 40 version-profile,
72 debug-info, three copy-owner, and three oracle tests. This source-family
milestone does not establish whole-project or corpus-wide compiler parity.

## C declaration provenance and IPA debug scopes, 2026-09-06

Five Twilight Princess GC/3.0a3 OSSync configurations now match their configured
reference objects byte for byte: **RZDP01, RZDE01_00, RZDE01_02, RZDJ01, and
DZDE01**. Across all 54 OSSync configurations, baseline `04857c6b` changes from
**44 BYTE, seven DIFF, three HARNESS** to **49 BYTE, two DIFF, three HARNESS**.
All 44 previous exact objects remain exact. Executable bytes and relocations
remain **50/51 measured exact**, with no empty objects. The two remaining DIFF
rows are the Wii Shield and ShieldD variants.

The declaration-name provenance pass now runs for C as well as C++. It keeps
source-written parameter names even when semantic parsing cannot finish a
pointer-to-array parameter, a function-pointer-return declarator, or an
assembly prototype. Prefix comparisons of the SDK headers isolated the final
14 missing ordinals to those forms; unrelated header prefixes already matched.
The fix preserves name provenance without inventing callable types.

A separate debug source-analysis plan models ordinary scalar locals and the
line-header frontier for straight-line C units. Without file IPA it discounts
later named parameters from the first header; with IPA it includes later body
scopes and local declarations. Closing scopes retain their emission order.
The driver passes the IPA setting explicitly to debug lowering. Control flow,
aggregate images, static locals, and C++ retain their existing ordinal owners.
Two new unit tests cover the IPA frontier/closing-scope distinction and the
GC/Wii const-local analysis cost. A parser test covers the recovered declarators
and unnamed controls.

A paired selection of **72 authored debug/inline canaries on GC/3.0a3 and
Wii/1.0** improves from **17/68 to 23/68 whole-object exact**, with no lost
matches. Of 144 build/canary slots, 76 are explicitly build-excluded; there are
no reference rejections or timeouts. Gains are 1538 and 1541 on both compilers,
1543 on Wii, and the new 1560 on GC/3.0a3.

Four new canaries cover the SDK's command-line IPA/catalog configuration
(1560), ordinary source-ordered scopes (1561), IPA scopes (1562), and recovered
C prototype names with unnamed controls (1563). Across eleven compiler builds,
they improve from **8/26 to 9/26 oracle-runnable whole objects**, with 18 build
exclusions across 44 slots. The preexisting source-pragma canary 1559 remains
nonexact: source-level `#pragma cats off` is still ignored, while the actual
SDK command-line option used by 1560 is modeled. Remaining canary line and
symbol differences stay in the corpus.

All **44 matrix configuration outcomes remain unchanged: 19 BYTE, 15 DIFF,
seven DEFER, three HARNESS**, including six empty exact objects. Compiler and
oracle builds pass. Debug-info tests pass **72/72**. Parser tests pass **401/403**;
the two failures are the previously recorded friend/template-layout and
discarded-inline aggregate-image tests, also present in the saved baseline
parser log. The new C provenance test passes. These are targeted measurements,
not a corpus-wide parity estimate.

## Modern inline symbols and debug containers, 2026-09-06

GC/3.0a3 and Wii/1.0 drop unused C inline-assembly helper symbols. Their
version profiles now select that retention policy while preserving references
to helpers that survive lowering. GC/3.0a3 also charges the same base analysis
cost for discarded plain-inline definitions as static-inline definitions.
Both ordinary and assembly-containing bodies expose the three-ordinal cost;
parameter and local costs remain separate existing policies.

The fragmented debug timeline now distinguishes ordinary C entry scopes from
assembly bodies. Ordinary functions create their line header after body
analysis; assembly bodies omit one entry-scope ordinal. Local function DIE
fragments are published with the completed unit, in debug-record order. An
explicit object-layout variant places the function catalog ahead of debug
sections when the first body has no anonymous payload; a pool-bearing first
body retains the earlier debug-section placement.

A paired regression selection of **67 authored filenames containing `inline`
or a `-sym on` directive**, run on GC/3.0a3 and Wii/1.0, improves from
**9/58 to 17/58 whole-object exact** against baseline `6d3bd276`, with no lost
matches. The 134 candidate/build slots include 76 build exclusions, zero
reference rejections, and zero timeouts. Gains are canaries 1314's C static
inline helper, 1536, 1537, and 1556 on both modern compilers. This selection
was measured before adding the separate IPA canary 1559.

Canary 1314's declared coverage now includes GC/1.3.2r, GC/3.0a3, and Wii/1.0;
it matches **11/11** measured builds. New canary 1556 covers static and plain
unused assembly helpers with debug enabled and matches **6/11**, up from
4/11 at baseline. New canaries 1557/1558 preserve dropped plain-inline
numbering and mixed assembly/C debug lines; both remain **0/11 exact** despite
matching modern debug ordinals. Their remaining line-table differences are
retained as coverage, not hidden by source formatting changes.

Canary 1559 reproduces the SDK's `-ipa file` vector-installer debug numbering
with exception metadata and the function catalog disabled. It is runnable on
GC/3.0a3 and Wii/1.0 and remains **0/2 exact**; both have matching text. The
nine older measured builds reject `-ipa` (verified as an unknown option), so
the canary explicitly declares those two applicable builds. Across all four
new canaries, **6/35 oracle-runnable objects match**, with nine declared build
exclusions across 44 build/canary slots.

All **54 configured OSSync outcomes remain 44 BYTE, seven DIFF, three HARNESS**,
with **50/51 measured executable sections and relocations exact**, no empty
objects, and no lost exact objects. RZDP01's remaining anonymous-numbering gap
includes the independently reproduced IPA line-header timing difference; full
header analysis still differs and remains open. Object (30), version-profile
(40), and debug-info (70) unit tests all pass, including a new assembly/C
ordinal-transition test. No corpus-wide parity claim is made.

## Ordering intrinsics and SDK vector installers, 2026-09-06

The shared intrinsic classifier now recognizes zero-argument `__sync`,
`__isync`, and `__eieio`. They emit target instructions, remain effectful, and
participate as scheduling barriers without introducing calls or LR saves.
Canary 1553 matches **11/11 whole objects**, up from **0/11** at `b48e4d2a`,
across GC/1.1, GC/1.1p1, GC/1.2.5, GC/1.2.5n, GC/1.3, GC/1.3.2,
GC/1.3.2r, GC/2.6, GC/2.7, GC/3.0a3, and Wii/1.0.

The existing semantic vector-copy owner now handles a destination returned by
an initializer call under the legacy frame convention. Under the later frame
convention it retains the fixed address's high half and regenerates the low
half at each call. Source-line emission follows the actual initializer/copy/
flush/barrier/invalidate boundaries. Static function debug fragment names now
use their emitted local-subroutine tag.

A paired run of **all 54 inventory configurations ending in `/OSSync.c`**,
using the configured reference object as the verdict input, changes:

| Layer | Baseline `b48e4d2a` | Candidate |
| --- | --- | --- |
| Whole-object BYTE | 43/54 | **44/54** |
| DIFF | 7 | 7 |
| DEFER | 1 | **0** |
| HARNESS | 3 | 3 |
| Executable bytes and relocations exact | 43/51 measured | **50/51 measured** |

There are no lost exact objects and no empty objects in this selection.
Twilight Princess ShieldD's GC/1.2.5n `OSSync.c` is the new whole-object match.
RZDP01's GC/3.0a3 object has matching code, text relocations, raw `.line`, and
raw `.debug`; debug symbol retention, placement, and anonymous ordinals still
prevent whole-object parity. The previous eight-row assembly/debug selection
improves from **six BYTE, one DIFF, one DEFER** to **seven BYTE, one DIFF**;
all eight now have exact code and relocations.

Canaries 1554/1555 retain helper-produced and constant destinations with debug
information and the oracle's default exception metadata enabled. Both are
runnable on all eleven builds and remain **0/22 whole-object exact**. Their
text matches on two and four builds respectively; remaining metadata and
scheduling differences are preserved as coverage. The older
`fixed_address_copy_barrier` canary stays **0/11 exact**, with no lost matches;
its GC/3.0a3 and Wii/1.0 text now matches. Across the three newly authored
canaries, **11/33 whole objects are exact**, with no build exclusions or
reference rejections.

Validation: compiler/oracle build passes; two intrinsic, three copy-owner,
69 debug-info, and 97 register/scheduler unit tests pass (eight existing
register/scheduler tests ignored). Checks remain focused on the affected
source family rather than claiming a fresh corpus-wide measurement.

## Focused scheduler progress and graph captures, 2026-09-06

The port-aware scheduler no longer waits indefinitely for an already-ready
operation that its own picker has declined. Waiting for a partner now requires
that partner to become available on the next cycle, and single-issue candidate
models do not wait for pairs. A producer selected in the current issue window
also releases an otherwise blocked independent load for that window. Legacy
integer load/arithmetic joins retain canonical commutative register slots after
allocation without changing DAG operand identities.

Canary 1551 reproduces the compiler hang through ordinary source: two global
stores, one fed by a multiply-plus-load and the other by a multiply. At baseline
`dc529ff5`, direct compilation exceeds a three-second timeout on GC/1.1,
GC/1.1p1, GC/1.2.5, and GC/1.2.5n; the other seven measured builds emit. The
candidate emits on all eleven and matches whole objects on **GC/1.1,
GC/1.2.5, and GC/1.2.5n**. GC/1.1p1's reference reverses the two stores relative
to those builds and remains nonexact. Modern schedules also remain different.
Canary 1552 retains reversed-source and XOR/load controls; it remains nonexact.

Canaries 1549/1550 vary floating reduction row count and layout. Both are
oracle-runnable on all eleven builds, with no exact objects. Across all four new
canaries, **3/44 whole objects are exact**, with no reference rejections or build
exclusions. The baseline timeouts are tracked separately, not counted as a
completed baseline oracle comparison.

A focused shared legacy DAG/store/float regression denominator remains **3/46
exact**, with no outcome changes. All **97 register/scheduler unit tests pass**;
eight existing tests are ignored. Three new tests exercise single-issue
progress, the already-ready load stall, and the measured integer issue order.
All 44 configured matrix outcomes remain **19 BYTE, 15 DIFF, seven DEFER,
three HARNESS**, including the same six empty exact objects.

The new `tools/float_snapshot_graph.py` diagnostic separates operation identity
from FPR coloring and instruction order. It rejects unsupported instructions,
preserves arithmetic operand order, gives each read its own identity, and
records memory dependencies across stores. Four Python tests cover those
boundaries. The committed capture under `harness/float_snapshot_schedules/`
records Wind Waker D44J01's GC/1.2.5n scalar matrix graphs and object hashes.
They contain the same operations as the candidate, with only **3/36** and
**4/30** at the reference positions. A 720-case latency/issue-model experiment
does not reproduce either full order; it is diagnostic evidence, not a validated
new floating scheduler. Matrix byte-parity and full-project parity remain open.

Local evidence: `target/reference-parity/d77092ee87d8d5ea-5e4ca1ddc460f4d8.jsonl`,
`target/bundler-baseline-compile.json`, `target/bundler-join-final.log`,
`target/snapshot-reductions-{baseline,final}.log`,
`target/bundler-regressions-{baseline,final}.log`,
`target/bundler-final-unit-tests.log`, and `target/snapshot-schedule-fit.log`.

## Focused floating snapshot load reuse, 2026-09-06

Promoted straight-line floating snapshots now reuse repeated single-precision
loads from source-proven ordinary pointer parameters. The parser retains that
memory fact separately from storage types; missing facts disable reuse. C/C++
volatile qualifiers, volatile typedefs, and aggregates containing volatile
members do not acquire ordinary-memory permission. Casts and unsupported source
or instruction shapes leave the previous lowering intact.

The pass assigns distinct virtual identities to definitions before sharing
loads. It preserves instruction order, clears every available load at each
store because parameters may alias, and uses the existing allocator and
instruction-index remapping. It inherits the preceding milestone's measured
version/optimization policy through aggregate-promotion provenance.

In Wind Waker D44J01, `C_MTXMultVec` improves from 172 to **148 bytes**, and
`C_MTXMultVecSR` from 148 to **124 bytes**. Both now equal the reference function
sizes after six redundant source loads are removed from each. Instruction
scheduling and register choices remain different, so neither is counted exact.
All eight functions still emit; the same four assembly functions remain exact
(444/1676 reference function bytes). Loop bodies retain their previous lowering.

All 44 configured matrix outcomes remain unchanged from `a6450b24`: **19 BYTE,
15 DIFF, seven DEFER, three HARNESS**. Thirteen of the exact objects are nonempty;
six are empty. No additional whole-object or full-project parity is claimed.

Canaries 1546–1548 cover ordinary shared inputs, volatile inputs, and an
intervening aliased output store. Across the same eleven compiler identities as
the preceding checkpoint, they improve from **0/33 to 9/33 whole-object exact**,
with every case oracle-runnable and no exclusions/rejections. All nine matches
are 1546 on GC/1.1 through GC/2.7; GC/3.0a3 and Wii/1.0 remain nonexact.
The volatile and intervening-store cases remain nonexact and stay in the corpus.

A focused shared denominator of **52 comparisons** across GC/1.2.5n, GC/1.3.2,
GC/2.6, and GC/3.0a3 improves from 18 to 21 exact. Only the three 1546 cases
change outcome. All six aggregate/frame integration tests pass, including
explicit volatile casts and reload placement after an aliased store. The parser
suite passes 400/402 with the same two established baseline failures.

Local evidence: `target/reference-parity/36e0f3172bededa0-5e4ca1ddc460f4d8.jsonl`,
`target/snapshot-load-{baseline,final}.log`,
`target/snapshot-regressions-{baseline,final}.log`,
`target/snapshot-matrix-detail.log`, `target/snapshot-integration-tests.log`, and
`target/snapshot-parser-tests.log`. Baseline executables were frozen from
`a6450b24` in `target/aggregate-promotion-baseline-bin/` before implementation.

## Focused private floating aggregate follow-up, 2026-09-06

Call-free functions can now promote private floating aggregate fields into scalar
locals, using the structured allocator. The transformation preserves statement
order and declines address exposure, volatile objects, overlapping non-float
views, assembly, and unsupported uses. Allocation finishes before an unused
provisional frame is removed. Version policy enables this at `-O4` for the
measured 2.x compiler line; GC/3.0a3 and Wii/1.0 retain their existing lowering,
since their reference outputs preserve aggregate stores in the new copy probe.

The parser carries volatile-member provenance through C and C++ layouts,
including nested aggregates, templates, and inheritance. Locals containing
volatile members stay memory-resident, including in the older in-place
aggregate scalarization pass.

New canaries 1544/1545 cover an aliased source snapshot and a parameter-to-field
copy. Across GC/1.1, 1.1p1, 1.2.5, 1.2.5n, 1.3, 1.3.2, 1.3.2r, 2.6, 2.7,
3.0a3, and Wii/1.0, whole-object matches improve from **0/22 to 9/22** against
the pre-promotion compiler artifact. All 22 are oracle-runnable, with no
exclusions or rejections. The nine matches are 1545 on all but the last two
builds. The snapshot still differs in instruction scheduling/register choices;
both later-build cases also remain nonexact.

All **44 configured `mtxvec.c` outcomes remain unchanged** from `ce5ddfcb`:
13 nonempty and six empty whole-object matches, 15 differences, seven deferrals,
and three harness failures. Wind Waker D44J01's `C_MTXMultVec` shrinks from
204 to 172 bytes against the 148-byte reference after its temporary stack
stores/reloads and unused frame disappear. Repeated source loads and scheduling
still differ. All eight functions remain emitted, with the same four assembly
functions exact (444/1676 reference function bytes). This is a lowering
improvement, not an additional exact project object or a full-project parity
claim.

A focused shared denominator of 40 canary/build comparisons across GC/1.2.5n,
GC/1.3.2, GC/2.6, and GC/3.0a3 improves from 15 to 18 exact; only the three
1545 cases change status. It covers aggregate, vector-struct, local-float,
materialized-return, and paired-single matrix probes. Existing failures include
four unsupported local-float cases and four source-encoding failures. Four
aggregate/frame integration tests pass, checking aliased read/store order,
volatile memory accesses, address exposure, and existing leaf frame behavior.
The parser suite passes 399/401; the same two baseline failures listed in the
preceding checkpoint remain.

Local evidence: `target/reference-parity/cf9e005ecd9e452f-5e4ca1ddc460f4d8.jsonl`,
`target/aggregate-scalar-{baseline,final}.log`,
`target/aggregate-{baseline-regressions,regressions,nearby,nearby-baseline}.log`,
`target/aggregate-matrix-detail.log`, `target/aggregate-parser-tests.log`, and
`target/aggregate-scalar-tests.log`. The baseline executables are the frozen
pre-promotion row-array candidate in `target/row-array-candidate-bin/`.

## Focused matrix row-array follow-up, 2026-09-06

Three configured matrix translation units improved from `DIFF` at baseline
`b871a334` to authoritative whole-object `BYTE`:

- Animal Crossing GAFE01_00 and GAFU01_00, GC/1.2.5.
- Ocarina of Time GC port `mq-j`, GC/1.2.5n.

Animal Crossing GAFE01_00 now matches its 308-byte `.text`, 768-byte `.line`,
and 868-byte `.debug`, including symbols and relocations. These are complete
object matches, not executable-section projections.

The parser retains the declaration identity, scalar identity, and row extent
behind decayed C/C++ and assembly array parameters. Independently declared array
typedefs remain distinct even with equal dimensions; aliases reuse the original
identity. General legacy debug lowering emits each type immediately before its
first consuming function while sharing previously emitted aggregate records.
The later fragment boundary walker also accepts types between functions.

A fresh comparison of all 44 configured `mtxvec.c` rows against the previous
commit gives:

| Outcome | Baseline | Current |
| --- | ---: | ---: |
| Whole-object exact, nonempty | 10 | 13 |
| Whole-object exact, empty | 6 | 6 |
| Different object | 18 | 15 |
| Compiler deferral | 7 | 7 |
| Harness failure | 3 | 3 |

Only the three rows above changed status. The six empty matches are two Mario
Kart Double Dash and four Metroid Prime configurations. The three harness
failures are Twilight Princess GZ2E01/GZ2J01/GZ2P01 Revolution matrix rows; they
remain unmeasured. The previous eight-row assembly-debug selection improves
from 4 to 6 exact, with one difference and one deferral. No full project has
been proven exact.

Canaries 1541/1543 cover declaration reuse, separate identical typedefs,
explicit array parameters, unsigned-long rows, and row-pointer typedefs. They
match **16/22** whole objects across the eleven compiler identities used in the
preceding checkpoint; later GC/2.7, GC/3.0a3, and Wii/1.0 debug objects still
differ. Canary 1542 exercises ordinary C row parameters: its instructions match
on all eleven builds, but debug locations/line records or fragment layout still
differ. Across all three new canaries the result is **16/33 exact**, with no
oracle rejections or exclusions. These failures remain in the corpus.

A shared debug-canary regression denominator across nine compiler identities
retains **26/46 exact**, with no outcome changes. All 69 debug unit tests pass.
The parser suite passes 398/400, including the new C/C++ declaration-identity
test; both failures reproduce at baseline (397/399):
`recovers_friend_bearing_layouts_and_expression_template_arguments` and
`retains_brace_initialized_aggregate_image_from_discarded_inline`.

Final clean-build evidence:
`target/reference-parity/376bec060fe1bc5d-5e4ca1ddc460f4d8.jsonl`.
Baseline matrix evidence:
`target/reference-parity/828b2f54d8cd2072-5e4ca1ddc460f4d8.jsonl`.
Additional local reports: `target/row-array-clean-oracle.log`,
`target/row-array-final-tests.log`, `target/row-array-baseline-parser-tests.log`,
and `target/row-array-{regressions,baseline}.log`.

## Focused typed assembly-parameter follow-up, 2026-09-06

Assembly signatures now use the ordinary type parser, retaining scalar typedef
identity, aggregate tags, and pointee qualifiers. They share the existing named
GPR binding path with embedded assembly. The assembler records incoming parameter
homes for debug lowering; const aggregate pointers carry both pointer and const
modifiers. Floating values remain on the separate FPR cursor, while pointers to
float consume GPRs. Legacy FPR location encoding, multiword/stack debug homes,
and decayed-array source types remain unfinished.

Animal Crossing GAFE01_00 now emits all ten formal-parameter records. Its
`.debug` grows from 428 to 792 bytes against the 868-byte reference; two matrix
row-array DIEs and their references/order still differ. The exact 308-byte
`.text` and 768-byte `.line` sections are preserved. This remains a whole-object
`DIFF`, with no additional reference-project parity credit.

Canaries 1538/1539 (typed scalar and const aggregate debug arguments) match
**16/22** whole objects across GC/1.1, 1.1p1, 1.2.5, 1.2.5n, 1.3, 1.3.2,
1.3.2r, 2.6, 2.7, 3.0a3, and Wii/1.0. The six failures are the two canaries on
GC/2.7, GC/3.0a3, and Wii/1.0: instructions match, debug object layout and
relocations do not. Canary 1540 (mixed double and float-pointer arguments)
matches **11/11** whole objects with debug disabled. All 33 comparisons were
oracle-runnable; none were excluded. At baseline `a3fdfac7`, GC/1.2.5 fails
both debug probes and rejects 1540's named pointer operand.

An assembly regression slice across GC/1.2.5, GC/1.3.2, and GC/2.6 preserves
all **60/71** previously exact/runnable comparisons. Eleven baseline failures
remain: nine source-encoding failures (three canaries on each build) and two
section/symbol mismatches in canary 1311. The debug crate's 69 tests and four
assembly integration tests pass.

The same eight-row reference selection as the preceding checkpoint remains
**4 BYTE / 3 DIFF / 1 DEFER / 0 unknown**, all nonempty. Wind Waker D44J01's
matrix spot check still emits all eight functions, with its four assembly
functions exact (444/1676 reference function bytes); C scheduling differences
remain. No full project has been proven exact.

Local evidence: `target/reference-parity/aa60ff6ab2af415e-5e4ca1ddc460f4d8.jsonl`,
`target/asm-debug-selection.json`, and the `target/asm-parameters-*.log` and
`target/asm-float-pointers-oracle.log` focused reports.

## Focused assembly line-provenance follow-up, 2026-09-06

The assembler now reports whether it appended a terminal `blr`. Legacy debug
lowering keeps the written instruction rows while leaving that implicit word
unmarked, except in GC/1.3 where the measured profile requires a line-zero row.
Void assembly bodies also bypass the ordinary empty-C-function line planner.
Generated frames still require their own source map; an unexplained instruction
count never receives a guessed instruction-level mapping.

Animal Crossing GAFE01_00's configured matrix `.line` section improved from
268 bytes to the exact **768-byte** reference section. Its 308 executable bytes
also remain exact. `.debug` still omits formal parameters and array types, so
the translation unit remains `DIFF` and earns no whole-object parity credit.

A final eight-row check combined both Animal Crossing matrix configurations
with six sampled `OSSync.c` configurations:

| Outcome | Configurations |
| --- | ---: |
| Authoritative whole-object exact, all nonempty | 4 / 8 |
| Different object | 3 / 8 |
| Compiler deferral | 1 / 8 |
| Measurement unknown | 0 / 8 |

The four exact rows are Pikmin GPIJ01_01, Pikmin 2 GPVE01_D17, Mario Party 4
GMPE01_00, and Metroid Prime GM8E01_01 `OSSync.c`. These are current exact
observations, not claimed gains over a prior fingerprint. The Twilight Princess
Revolution row differs, and the ShieldD Dolphin row defers on its debug
vector-installer plan.

The debug crate's 69 unit tests pass. Canaries 1536/1537 match whole objects in
20/22 build comparisons across eleven compiler identities. Their two GC/2.7
objects retain matching `.text`, `.line`, and `.debug` bytes but differ in debug
fragment symbols, relocation targets/kinds, and `.comment`; those comparisons
remain failures. No oracle invocation was rejected.

Local evidence and selection:
`target/reference-parity/03ca4e6a17178bcf-5e4ca1ddc460f4d8.jsonl` and
`target/asm-debug-selection.json`.

## Focused legacy leaf-frame follow-up, 2026-09-06

Compiler milestone `7dc50475`, compiler hash prefix `af32970857558c0d`.

The next iteration selected the 14 matrix configurations previously stopped by
`inlined leaf has an unexpected linkage frame`: four Wind Waker, one Melee,
and nine Twilight Princess Dolphin configurations, all using GC/1.2.5n.

All **14/14 now emit complete objects**, changing `DEFER -> DIFF`. Whole-object
exactness remains **0/14**, and measurement unknown is **0/14**. This is a
compilation-coverage gain, not an exact-parity gain. Wind Waker's D44J01 object
now contains all eight expected functions, including the four C matrix routines
that were previously omitted from the partial diagnostic projection. Its four
assembly functions remain exact (444/1,676 reference function bytes); the C
routines expose aggregate-temporary, allocation, and scheduling differences.

Frame cleanup now recognizes both existing prologue conventions. It removes
only the LR save/restore pair from a proven leaf, preserving the allocation and
all live frame-local accesses. The regression compiles a volatile aggregate
leaf across GC/1.1, 1.1p1, 1.2.5, 1.2.5n, 1.3.2, and 2.6 and checks balanced
stack storage without LR traffic; a real-call linkage regression also passes.
Canary 1535 records the remaining instruction-order and register differences
against the GC/1.2.5n oracle and is not yet whole-object exact.

Local evidence is retained in
`target/reference-parity/af32970857558c0d-5e4ca1ddc460f4d8.jsonl` and
`target/leaf-frame-selection.json`. Reproduce the focused set with:

```sh
python3 tools/reference_parity.py --compiler target/debug/mwcc \
  --selection target/leaf-frame-selection.json --timeout 30 --jobs 6 --code-projection
```

The Animal Crossing matrix object was inspected separately: its configured
executable bytes and relocations match, but `.debug` omits formal parameters
and array types, and `.line` falls back to function boundaries for implicit
assembly returns. Those debug differences remain on the frontier.

## Focused alias follow-up, 2026-09-06

After the paired-single milestone `991ad4e3`, the next iteration selected the
14 `subi`-blocked rows plus the six Mario Party 4 matches for regression checks.
All 20 were rerun against compiler hash prefix `43ca92bf265dabc8` and
harness hash prefix `5e4ca1ddc460f4d8`:

| Outcome | Configurations |
| --- | ---: |
| Authoritative whole-object exact, all nonempty | 10 / 20 |
| Different object | 3 / 20 |
| Compiler deferral | 7 / 20 |
| Measurement unknown | 0 / 20 |

Both Pikmin 2 variants (GC/1.2.5n) and both Sunshine variants (GC/1.2.5) moved
from compiler deferral to whole-object exact. All six Mario Party 4 variants
remain exact. Animal Crossing's two configurations now have exact executable
bytes and relocations but different objects, so neither earns parity credit.
Twilight Princess ShieldD's Dolphin object also differs; its seven selected
Revolution objects defer on fragmented/interleaved debug-info emission.

`subi` and `subis` now lower to structured add-immediate instructions with a
negated field. Profile-controlled validation preserves the measured difference
at -32768: 2.3.x rejects it; 2.4.x and later wrap it. The ordinary alias canary
matches whole objects on 14 identities; the wrapping-boundary canary matches
on the ten applicable later identities and explicitly excludes the four older
builds. The assembler's 15 focused unit tests pass.

These results concern a failure-selected subset, not an overall parity estimate.
The local result and selection files are
`target/reference-parity/43ca92bf265dabc8-5e4ca1ddc460f4d8.jsonl` and
`target/subi-validation-selection.json`. The command below reruns the complete
44-row parent matrix when a broader checkpoint is useful.

## Preceding matrix assembly checkpoint, 2026-09-06

Compiler milestone `991ad4e3`, compiler hash prefix `d797c161cc4f5c3a`.

The local inventory now contains **49,436 configured translation units**. A
failure-driven check selected all 44 configured `mtxvec.c` rows; it is not a
representative corpus estimate and does not replace the historical holdout below.

| Outcome | Configurations |
| --- | ---: |
| Authoritative whole-object exact | 12 / 44 |
| Different object | 1 / 44 |
| Compiler deferral | 28 / 44 |
| Harness/measurement unknown | 3 / 44 |

Six exact rows contain code: every Mario Party 4 variant's GC/1.2.5 matrix/vector
object, each with four functions and 444 executable bytes. The other six exact
rows are empty objects (two Mario Kart and four Metroid Prime configurations);
they demonstrate no matrix code-generation coverage. The original Mario Party 4
GMPE01_00 probe deferred on `ps_merge00` before this change.

The shared assembler now supports all four paired merge forms, scalar-lane
multiply and multiply-add, and quantized load/store with base update. Register
visitation models the base's read and definition; update memory operations are
scheduler barriers. GC/1.3's negative-displacement bug is reproduced through a
profile setting: negative PSQ offsets force W=1/I=7. Direct oracle measurements
confirm that GC/1.1, 1.1p1, 1.2.5, 1.2.5n, and 1.3.2 do not have this bug.

Remaining rows expose `subi` assembly aliases, retained inline functions with
linkage frames, and object metadata differences. The three harness unknowns are
Twilight Princess GC/3.0a3 configurations whose oracle invocation rejects the
source; no compiler parity credit is assigned to them.

Reproduce using the host-tool overrides documented in the README (macOS wibo
1.2.0, gc-wii-binutils 2.42-2, direct ASCII compilation with no sjiswrap):

```sh
cargo build -p mwcc
python3 tools/reference_parity.py --compiler target/debug/mwcc \
  --source 'mtxvec.c$' --timeout 30 --jobs 6 --code-projection
```

The completion proof and statistical checkpoints below retain their historical
47,879-configuration denominator. No project matrix has been proven complete.

## Completion proof

The configured corpus contains 47,879 translation units across 13 MWCC-configured
projects. All 13 compiler identities in the corpus are recognized. No project
matrix is complete yet. The latest fully measured fingerprint directly observed
411 configured TUs and proved 41 whole-object exact: 30 statistical sample rows
plus 11 breadth sentinels. Old-fingerprint exact observations are not counted as
proof about a newer compiler, so the literal proof at that measured fingerprint is:

| Measure | Result |
| --- | ---: |
| Configured TUs proven whole-object exact | 41 / 47,879 |
| Project matrices proven complete | 0 / 13 |
| Directly observed configurations at this fingerprint | 411 / 47,879 |

`fzerox` is the fourteenth discovered project but currently has no MWCC configure
metadata, so it is outside the 47,879-TU denominator. GC/1.3.2r is intentionally
not a required parity identity.

## Current untouched-frame holdout

Compiler commit `c0962f28` was frozen before membership was revealed. The
harness excluded every configuration ID present in any prior result cache:
1,639/47,879 configurations. A simple random sample without replacement drew
384 rows from the remaining untouched frame of 46,240 configurations (96.6% of
the configured corpus), using seed `mwcc-fresh-holdout-20260723-c0962f28` and
purpose `fresh-holdout`. Another 27 out-of-estimator sentinels covered all 13
compiler identities and all 66 project x compiler-version x language cells.

| Whole-object outcome | Count | Share of 384 |
| --- | ---: | ---: |
| Exact | 30 | 7.8% |
| Confirmed non-parity (`DIFF` or compiler `DEFER`) | 190 | 49.5% |
| Measurement unknown | 164 | 42.7% |

The confirmed exact share is 7.8%, with a 95% confidence interval of
5.5%-10.9%. The untouched-frame intrinsic identification range is 7.8%-50.5%:
the lower endpoint treats every unknown as non-exact and the upper endpoint
treats every unknown as exact. Conservatively giving the excluded prior-observation
stratum no current credit at the lower endpoint and full credit at the upper
endpoint produces a full-corpus range of 7.5%-52.2%.

Unknown attribution is 94 60-second timeouts, 62 missing dependencies, and 8
invalid captured configurations. A compiler `DEFER` is confirmed non-parity,
not measurement unknown. Among rows with resolved authoritative outcomes, the
conditional exact rate is 30/220 (13.6%); it is not the headline estimate.

Of the 36 sample rows that emitted objects, 30 were exact and six differed
(83.3% conditional exactness). Code plus text-relocation evidence was exact for
8/12 measured objects. Relocation-aware diagnostics covered 12 objects:
14/30 reference functions and 964/3,356 reference code bytes were exact. These
conditional diagnostics do not earn whole-object parity credit.

Ten whitespace-only rows account for ten of the whole-object exact results. On
the 374 substantive-source rows, 20 were exact (5.3%). This is reported
separately so empty translation units cannot make compiler capability look
better than it is.

The run took 897.9 seconds of active wall time and 6,543.8 aggregate row-seconds.
Median row time was 1.62 seconds; p95 and maximum were approximately 60 seconds.
This validates the failure-only edit loop: representative audits are useful
periodic measurements, but recompiling them continuously would spend most of
its time on known giant-TU timeouts.

Post-holdout compiler work through commit `d2609aad` has not been run over a
new unbiased sample, so it does not change the 7.8% estimate above. On the
targeted Melee `src/melee/ft/ftcommon.c` diagnostic, the latest checkpoint moved
relocation-aware parity from 28/109 to 82/109 functions and from 996/15,340 to
7,600/15,340 reference code bytes. Paired movement was +54/-0 functions and
+6,604 exact bytes. Of 83 comparable functions, 82 were relocation-aware exact;
one emitted mismatch and the other 26 remained compile/defer coverage gaps. The
gains were `ftCommon_ClampAirDrift`, `ftCommon_FallBasic`,
`ftCommon_CalcHitlag`, `ftCommon_8007DB58`, `ftCommon_SetAccessory`,
`ftCommon_8007FF74`, `ftCommon_8007DB24`, `ftCommon_8007D28C`,
`ftCo_GetLStickAngle`, `ftCo_GetCStickAngle`, `ftCommon_8007D780`,
`ftCommon_8007F9B4`, `ftCommon_8007E2A4`, `ftCommon_8007E690`,
`ftCommon_ApplyGroundMovementNoSlide`, `ftCommon_ApplyFrictionAir`,
`ftCommon_8007EF5C`, `ftCommon_8007CDA4`, `ftCommon_8007CDF8`,
`ftCommon_8007D5D4`, `ftCommon_CheckFallFast`, and the twin decay functions
`ftCommon_8007CCA0` and `ftCommon_8007CE4C`, plus the file-IPA sign store
`ftCommon_8007DA24` and the guarded ground projection `ftCommon_8007CCE8`.
The retained derived-member lifetime also made `ftCommon_8007D60C` exact, and
the symmetric returned decay made `ftCommon_8007CD6C` exact. Retaining the
member velocity and pooled zero through target-acceleration clamps made the twin
functions `ftCommon_8007CA80` and `ftCommon_8007D2E8` exact. Member-backed
friction selection made `ftCommon_8007CEF4` exact; retaining the inlined clamp,
shared reset zero, and split-address report schedule made `ftCommon_8007D7FC`
exact. Limiting legacy inline frame residue to call-bearing helper bodies then
made the standalone transition `ftCommon_8007D6A4` exact. Modeling unused
scratch arrays as frame bytes without value-table lanes, canonicalizing
saved-GPR slots, and retaining the cross-alias zero made
`ftCommon_ApplyGroundMovement` exact. Retaining pointer-member element strides,
deduplicating aggregate/scalar frame pressure, keeping nullable frame addresses
in r4, sharing adjacent leading zero literals, and scheduling the resulting
frame-vector producer windows made `ftCommon_8008021C` exact. Extending automatic
inlining to terminal helpers that select through mutable scalar parameters,
then retaining their acceleration, target, velocity, and zero lanes through
the nested clamp made `ftCommon_8007CADC` and `ftCommon_8007D3A8` exact. A
structured two-arm friction owner then made `ftCommon_8007CF58` and
`ftCommon_8007D050` exact. Scheduling the full friction-bounded acceleration
region made `ftCommon_8007D174` exact. Generalizing that semantic owner to keep
a member-backed velocity live through the ladder then made
`ftCommon_8007C98C` exact. Scheduling the complete three-guard joystick-count
transaction made `ftCommon_8008031C` exact. Reusing the null-tested entry from
indexed global callback tables then made `ftCommon_8007E79C`,
`ftCommon_8007E7E4`, `ftCommon_8007F578`, `ftCommon_8007F824`, and
`ftCommon_8007F86C` exact. Allocating wide masks and loaded float comparisons,
extending narrow leaf/member comparisons, and scheduling shared-base bit-field
call arguments then made `ftCommon_GrabMash` compile completely. The complete
cross-statement transaction schedule then made it byte- and
relocation-exact. Finally, preserving every arm-live parameter across a call in
an if-condition repaired the stale-r3 miscompile in `ftCommon_8007ECD4` and made
that function byte- and relocation-exact. Giving the single call-bearing
conjunction a value-origin frame lane and MWCC's saved-argument issue order then
made `ftCommon_8007EBAC` byte- and relocation-exact. Reusing a dying parameter
register for an inline local loaded from one of its members, then pooling a
repeated floating zero across the store run, made `ftCommon_8007E2FC` and
`ftCommon_8007E82C` byte- and relocation-exact. Expanded short-circuit list-walk
lowering then made `ftCommon_8007EC30` exact; paired negated and absolute-value
product lowering made `ftCommon_8007F7B4` and `ftCommon_8007F76C` exact. The
remaining emitted mismatch is `ftCommon_8007E0E4`; linkage-first saved-FPR
frames made it comparable, and true-edge float caching subsequently removed
three redundant instructions from its opening conjunction, but it is not
counted as an exact gain. This is evidence of local forward movement, not a
corpus-level percentage.

## Historical baseline: fresh current-population holdout

Before revealing membership, commit `10024016` was frozen and a simple random
sample without replacement was drawn from all 47,879 configured TUs. The sample
used seed `mwcc-representative-audit-v1`, epoch `2026-07-22-status-1`, and purpose
`fresh-holdout`. All 384 statistical rows completed. Another 30 out-of-estimator
sentinels exercised every project x compiler-version x language cell; they do not
affect the following rates.

| Whole-object outcome | Count | Share of 384 |
| --- | ---: | ---: |
| Exact | 35 | 9.1% |
| Confirmed non-parity (`DIFF` or compiler `DEFER`) | 195 | 50.8% |
| Measurement unknown | 154 | 40.1% |

The exact-within-protocol share was 9.1%, with a finite-population 95% confidence
interval of 6.6%-12.4%. This was the prior whole-population holdout and remains
useful historical evidence. If every unknown
row were non-exact the intrinsic share would be 9.1%; if every unknown row were
exact it would be 49.2%. That 9.1%-49.2% identification range is intentionally shown
instead of guessing through missing evidence.

Unknown attribution is 99 harness/time-budget failures, 41 missing dependencies,
and 14 invalid captured configurations. A compiler `DEFER` is not unknown: it is
confirmed non-parity. Of the 42 sample rows that emitted an object, 35 were exact
and 7 differed (83.3% conditional exactness). That conditional number is useful
for backend diagnosis but must not be presented as feature or corpus coverage.

Relocation-aware diagnostics were available for 12/384 sample objects: 28/49
reference functions were exact and 840/3,984 reference code bytes were exact.
These diagnostics do not earn whole-object parity credit.

## Latest paired checkpoint

Compiler commit `869596ad` was run over the exact 384-row untouched-frame sample
that was first revealed at `018cffe0`, plus its 29 out-of-estimator breadth
sentinels. Membership is now known, so this is a paired movement measurement,
not a new unbiased estimate of the current compiler.

| Whole-object outcome | `018cffe0` | `869596ad` | Change |
| --- | ---: | ---: | ---: |
| Exact | 31 / 384 | 33 / 384 | +2 |
| Confirmed non-parity (`DIFF` or compiler `DEFER`) | 193 / 384 | 187 / 384 | -6 |
| Measurement unknown | 160 / 384 | 164 / 384 | +4 |

The current panel is 8.6% exact, with a descriptive 95% interval of 6.2%-11.8%
and an exact-or-unknown identification range of 8.6%-51.3%. Conservatively
projecting the untouched frame over the excluded prior-observation stratum gives
a full-corpus identification range of 8.4%-52.5%. The still-unbiased status
estimate remains the 8.1% fresh-holdout result recorded above.

Among 220 rows with authoritative resolved outcomes at both fingerprints,
whole-object exact movement was +2/-0. The exact gains were:

- `super_mario_sunshine/src/JSystem/JStage/JSGObject.cpp` (`DIFF -> BYTE`)
- `metroid_prime/src/Kyoto/Particles/CElectricDescription.cpp` (`DIFF -> BYTE`)

`twilight_princess/src/SSystem/SComponent/c_m3d_g_vtx.cpp` changed
`DIFF -> DEFER`, so it remains confirmed non-parity. Unknown attribution is 111
60-second timeouts, 41 missing dependencies, and 12 invalid configurations.

Of the 35 statistical rows that emitted objects, 33 were whole-object exact and
two differed. Relocation-aware diagnostics covered 12 objects: 49/51 reference
functions and 4,380/4,792 reference code bytes were exact. These are conditional
backend-quality diagnostics and earn no additional whole-object parity credit.

The run compiled every row at a 15-second cap, then retried only the 130 initial
timeouts at 60 seconds. It took 653.8 seconds of active wall time and 7,900.7
aggregate row-seconds; median row time was 2.38 seconds and p95 was 60.01 seconds.

Compared directly with the preceding `7c7f881e` paired checkpoint, this is +1
exact and -0 exact regressions among 220 jointly resolved rows. The gain is the
CElectricDescription sample row. Seven prior compiler `DEFER` rows instead hit
the 60-second ceiling, so parity moved forward while measurement precision moved
backward. Outside the estimator, Melee `src/MetroTRK/msg.c` also changed
`DIFF -> BYTE` as a breadth-sentinel gain.

Post-checkpoint commit `0aeceac7` re-armed the already measured Pikmin 2 UART
writer family and directly proved both configured variants whole-object exact.
Those targeted observations are not folded into the 384-row checkpoint above.

## Historical paired checkpoint at `93db2a25`

Compiler commit `93db2a25` was run over the exact same 384 statistical rows and
30 breadth sentinels. Because this panel's membership was known during compiler
work, it measures movement on the frozen panel; it is not a new unbiased
current-population estimate.

| Whole-object outcome | Baseline | Current | Change |
| --- | ---: | ---: | ---: |
| Exact | 35 / 384 | 40 / 384 | +5 |
| Confirmed non-parity (`DIFF` or compiler `DEFER`) | 195 / 384 | 183 / 384 | -12 |
| Measurement unknown | 154 / 384 | 161 / 384 | +7 |

The paired panel's exact share was 10.4%. Its descriptive finite-population 95%
interval is 7.7%-13.9%, and its exact-or-unknown identification range is
10.4%-52.3%. These describe the tuned historical panel and do not supersede the
current untouched-frame holdout. Among the 223 rows with authoritative,
resolved outcomes at both checkpoints, whole-object exact movement was +5/-0.

The five gains were `DIFF -> BYTE` transitions in:

- `super_smash_brothers_melee/src/melee/ft/chara/ftCommon/ftCo_ThrownKoopa.c`
- `super_mario_sunshine/src/MarioUtil/RumbleData.cpp`
- `wind_waker/src/PowerPC_EABI_Support/Runtime/Src/GCN_mem_alloc.c`
- `metroid_prime/src/MetroidPrime/CBallFilter.cpp`
- `ocarina_of_time_gc_port/src/metrotrk/mutex_TRK.c`

`twilight_princess/.../ut_TagProcessorBase.cpp` changed `DIFF -> DEFER`: its
14/14 functions and 2,316/2,316 code bytes are now relocation-aware exact, but
legacy DWARF emission still defers, so it earns no whole-object exact credit.
Seven Pikmin 2 rows changed `DEFER -> HARNESS` by exhausting the time cap; that
accounts for the seven-row increase in measurement unknowns.

Of the 41 statistical-sample rows that emitted objects, 40 were whole-object
exact. Relocation-aware diagnostics covered 11 objects: 34/35 reference functions
and 1,476/1,668 reference code bytes were exact. Code plus text-relocation shape
and targets were exact for 10/11 measured objects. These remain conditional
backend diagnostics, not feature-coverage estimates.

Unknown attribution is 106 harness/time-budget failures, 41 missing dependencies,
and 14 invalid captured configurations. Of the 106 harness unknowns, 104 hit the
300-second cap and two were non-authoritative rejected comparisons. The run took
2,468.0 seconds of active wall time; median row time was 2.48 seconds, p95 was
300.01 seconds, and the maximum was 300.04 seconds.

## What the audit says to work on

The largest sampled compiler blocker families were:

| Family | Sample rows |
| --- | ---: |
| C++ types, layout, and calls | 52 |
| Backend lowering, registers, and scheduling | 29 |
| Other unsupported lowering | 29 |
| Control flow | 20 |
| Front end, parsing, and resolution | 18 |
| Data and global initialization | 15 |
| ABI and runtime semantics | 9 |
| Inline expansion | 4 |
| Debug info and object format | 3 |
| Inline assembly | 3 |
| Emitted-object mismatches | 1 |

The latest measurement took 2,468.0 seconds of active wall time. Median row time
was 2.48 seconds, while p95 and maximum were approximately 300 seconds. Large
Twilight Princess and Wind Waker translation units exhausted the 300-second cap,
accounting for most of the 106 harness unknowns and most audit wall time. Making
those units reach a precise compiler diagnostic quickly, plus repairing missing
dependencies and invalid configurations, will narrow the status interval more
than increasing the random sample size today.

## Iteration and reporting contract

- Inner-loop work draws from a failure-biased queue. Previously exact rows do not
  consume the default budget; a regression simply re-enters the queue. Its
  default per-row cap is 60 seconds.
- A fixed paired panel is run only at explicit checkpoints to measure movement.
  Its default per-row cap remains 300 seconds.
- A fresh holdout whose membership was not inspected before freezing the compiler
  is used for an unbiased current-population estimate.
- The exhaustive 47,879-TU matrix is the only completion proof. Sampling estimates
  progress; it cannot declare the goal complete.
- Every status update states the numerator, denominator, outcome semantics, and
  unknown count. Undenominated "green/red" totals are harness telemetry, not parity.

Reproduce this checkpoint with:

```sh
python3 tools/parity_loop.py \
  --audit-only \
  --audit-size 384 \
  --audit-epoch 2026-07-23-unseen-018cffe0 \
  --audit-purpose fresh-holdout \
  --jobs 14 \
  --reference-root /path/to/reference_projects
```

Results are keyed by the compiler+harness fingerprint, so running this command
after a compiler or harness change creates a different checkpoint rather than
silently mixing observations.
