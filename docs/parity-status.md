# Reference-project parity status

Last fresh holdout: 2026-07-23 22:53 UTC at compiler commit `c0962f28`

Latest paired checkpoint: 2026-07-23 17:44 UTC at compiler commit `869596ad`

Latest targeted checkpoint: 2026-09-07, two computed pointer loads and array shadowing (fingerprint below)

Latest measured compiler + harness fingerprint: `a2fac7fe37e388a1a73915a18dc7e77d8187d98b4632666f396bf9d2c5a8fb7f:5e4ca1ddc460f4d86cd15e9e7a834f5b2a572a7e0c80e09279a629da4eac0806`

This file records a measurement checkpoint, not a claim that the numbers stay
current after compiler or harness changes. Canary and work-queue counts are
labeled diagnostics; neither is a corpus parity estimate.

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
