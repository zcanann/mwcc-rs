# Reference-project parity status

Last fresh holdout: 2026-07-23 22:53 UTC at compiler commit `c0962f28`

Latest paired checkpoint: 2026-07-23 17:44 UTC at compiler commit `869596ad`

Latest targeted checkpoint: 2026-09-06, combined EXI status transactions (fingerprint below)

Latest measured compiler + harness fingerprint: `661c31825be5e19c15d8015d031c4144219cefb840a02dcc10da87bd85412d26:5e4ca1ddc460f4d86cd15e9e7a834f5b2a572a7e0c80e09279a629da4eac0806`

This file records a measurement checkpoint, not a claim that the numbers stay
current after compiler or harness changes. Canary and work-queue counts are
labeled diagnostics; neither is a corpus parity estimate.

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
