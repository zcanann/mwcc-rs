# The mwcc -O4 emission model (the seam rules)

Measured across fires 419-433 (canaries 1067-1069 + the e_fmod knit
target, docs/efmod-knit-target.dis). These rules ARE the spec for the
general statement walker: statements compile independently given a
(live-set, free-registers, return-home, framedness) context, and the
seams between claimed shapes are concatenation plus this bookkeeping.

## Composition

- **Concatenation**: a scaffold prefix, a loop in an if-arm, a diamond
  in a diamond — each claimed segment's skeleton emits VERBATIM in its
  slot (fires 425/426; verified against the whole e_fmod at fire 433).
- **Returns**: FRAMELESS functions return inline per arm (`li; blr` —
  even mid-loop, fire 422). FRAMED functions branch to ONE shared
  epilogue (`b JOIN ... addi r1; blr`), and early returns load f1/r3
  then `b EPILOGUE` (fires 429/430, e_fmod).

## Registers

- Params in their EABI homes; renames alias in place (mr only when a
  loop-carried init needs it).
- r0: the scratch for branch-free values — BUT a long-lived value may
  OWN r0 across the whole function (e_fmod's sx: claimed by the first
  scaffold fold, held ~200 instructions; every later segment allocates
  AROUND it — the normalize bound hoists to r5, not r0). r0 also takes
  sequential dead-range reuse (hx then ly, fire 429) and serves as the
  DIAMOND JOIN register (all arms converge their result in r0 when it
  feeds one post-join consumer, fire 431).
- Loop/segment locals: freed count home first (mtctr kills it), then
  next-free ascending, in def order (fires 421, 425 — a live scaffold
  local shifts the pool).
- A computed value takes a DYING param's home (n into ix via subfic,
  fire 431; hz into hy's home, fire 423).

## Scheduling

- Loads: first-use order, each as LATE as its consumer allows — a load
  slots into a compare->branch latency gap (ly after the cmpw, fire
  429; the borrow cmplw hoists ABOVE the subtracts it precedes, fire
  421).
- Stores: by OPERAND READINESS, not source order (the LO store before
  the HI whose or-chain is still computing, fire 432; source order
  when both are ready).
- Spills (stfd) delay into independent int computation (fire 432).
- A loop step's constant decrement interleaves into add latency
  (fires 420/424); the low-word doubling sits in the srwi's shadow.

## Folds (int)

- `x & LOWMASK` -> clrlwi; `x & 0x80000000` -> clrrwi 31; `| HI` (low
  half zero) -> oris; `- HI` -> addis with the negated high half.
- `K - x` -> subfic; `x - K` -> addi -K; `(u)x >> 31` shifted by 3 ->
  one rlwinm (fire 428).
- Compare CSE: ONE cmplw serves multiple tests when CR0 survives the
  branches between them (fire 427). A subtraction feeding `< 0` fuses
  to the record form (subf./addic.) ONLY if nothing intervenes between
  def and test (fires 419-421).

## Loop regimes

- Constant-trip pointer fill: version-selected CTR batches or full expansion
  (see below). Other counted straight-line bodies remain deferred.
- Counted + branchy body: plain CTR loop (mtctr; skip-branch mirroring
  the entry test exactly; bdnz). `while(n--)` skips only on zero.
- Non-counted: the rotated form (b TEST; BODY; TEST; b<cond> BODY),
  big bounds hoisted lis BEFORE the loop (r0 unless owned).

## The knit target

docs/efmod-knit-target.dis is the real __ieee754_fmod (marioparty4,
2.6): 207 instructions, EVERY segment a claimed template shape with
context registers. Unprobed remainders: the y-NaN idiom
(`(ly|-ly)>>31` -> neg/or/srwi), the float purge (lfd/lfd/fmul/fdiv),
and the subnormal-output three-way (n<=20/31 with sraw + mr from r0).
The knit driver = these seam rules + a whole-function liveness
allocator that reproduces long-lived r0 ownership.

## Unit-layout address finalization

Complete BSS-relative addresses cannot always be encoded during function
selection: the final offset depends on version-specific declaration and
reference order across the whole translation unit. `mwcc-object::data_layout`
provides the canonical routing and BSS order. Before debug lowering,
`mwcc-machine-code-to-object::finalize_bss_addresses` resolves selected
`SymbolAddress` fixups against that layout and inserts an adjusted high half
when the complete displacement does not fit signed 16 bits.

The destination normally holds its own high half. An r0 destination needs a
separate volatile GPR because r0 is not a register base in `addi`; physical
CFG liveness selects a free home without clobbering live values. Opaque assembly
and exhausted scratch registers produce diagnostics when expansion is needed.
Control-flow labels land on the inserted high half, while instruction-owned
fixups remain attached to the low instruction. Debug lowering sees the final
instruction stream. The low symbol fixup remains for object emission and
reference ordering, but its complete-address marker is consumed.

Ordinary `Symbol` displacements remain low-half-only, so owners that already
bias a section page are not expanded twice. This pass currently resolves BSS;
initialized-data layout and scheduling the new high instruction are follow-up
work. Canaries 2076–2081 and BfBB `__AXVPBInit` exercise this boundary.

## Constant-trip pointer fills

`fixed_fill_loops` runs after ordinary instruction scheduling and before
allocation. Its proof requires a single-entry countdown, one constant integer
store, a matching pointer advance, and a dead final CR0 result. CFG liveness
keeps observable pointer/counter exit values. Other CTR owners, opaque assembly,
extra body entries, and body-owned relocations/displacements exclude the pass.
The ordinary instruction-edit helpers keep branches, labels, and metadata in
sync, and a fresh virtual value supplies the trip count.

`FixedFillLoopStyle::DivisorTen` selects the largest exact divisor up to ten
stores. `PacketEight` fully expands fewer than 64 stores; longer fills divide
complete eight-store packets using the largest divisor up to seven packets,
then emit the scalar remainder. Size mode selects one store per CTR iteration.
Explicitly aligned global references select the divisor policy for the entire
function, including unrelated parameter fills, on the packet-style builds.
O3/O4 enable this owner; disabled instruction scheduling does not disable it.

The linkage-first prologue's latency slots must remain above every branch
entry. Moving a loop-entry value above a delayed stack update also moves the
backedge there, incorrectly allocating another frame per iteration. The plain
frame scheduler now treats those entries as a boundary. Canaries 2082–2088
exercise both loop expansion and this frame-composition requirement.

## Shared address halves in narrow stores

`split_address_stores` builds a register-value graph within straight-line
halfword-store regions before allocation. It follows copies, signed 16-bit
address additions, and logical shifts by 16. Equal address/high-half values
share virtual homes; a zero-offset low half uses its input register directly.
The proof rejects mutated inputs or store bases, escaping scratch/return homes,
body-owned symbolic fixups, and opaque/indirect control flow. Branch entries
split regions. Store aliases do not invalidate these register-only values.

At O2 or with scheduling disabled, values materialize on first use. O3/O4
materialize later independent values after the first store. GC 3/Wii can
exchange the first matching high/low stores in a complete ordinary leaf when
both target disjoint fields of the same nonvolatile base. The version policy
is independent of value recognition; volatile stores retain source order.
Canaries 2089–2098 and the full BfBB AX initializer exercise this owner.


## Global-array pointer induction

`structured_global_array_cursors` exposes a leading `p = &array[i]` binding as
an ordinary loop-carried pointer at O2+. Its proof owns source relationships:
a private zero-based unit-step integer index with a positive constant bound,
a real unshadowed global array, an equal pointer stride, and a private pointer
that is neither rebound in the body nor observed after the loop. Indirect
stores do not rebind their base pointer. Escapes, volatile bindings, alternate
entries, and unsupported control flow decline the plan. Initializers and typed
increments feed the existing structured CFG and allocator, with no physical
register assignments or function-name recognition. Schedule-off still applies
the induction rewrite; O0 preserves the original source form.

Entry scheduling must examine incoming branch targets, including backedges
located after the first call. Argument constants and split masks cannot move
across those entries. A zero store can retain the existing loop-invariant
schedule only when the backedge preserves r0; call clobbers or other r0
definitions require materializing zero inside the loop. Canaries 2099–2103
exercise both source-derived and explicitly written cursors. The complete AX
initializer now carries its four pointers, while modern reference offset
induction and the remaining allocation/inlining schedules are still pending.

## Pointer arguments within an inline instance

A pointer may change between calls while remaining private during each call.
Statement composition distinguishes that property from function-wide scalar
stability: unescaped automatic pointer locals and pointer parameters can be
substituted, while globals and escaped locals are captured in hygienic parameter
lanes at the call site. Writes to the callee parameter itself still require a
lane. Pointer-variable casts preserve identity; other materialized address
expressions retain their established admission path. Opaque assembly prevents
private-pointer substitution.

Measured automatic inlining also substitutes volatile pointer variables at each
use, repeating their reads. Canaries 2104–2108 establish 11 reads per iteration
for the inlined reset plus callback. This is an original compiler quirk to
preserve. Wii/1.0 O0 retains the helper call and is a remaining threshold gap.

The existing member-value graph can own an interior straight-line run as well
as a complete leaf. Only r0 value definitions and stores through one unchanged
member base qualify; r0 must be dead on exit. Stores from other registers
through that same base can remain in the graph as independent stores. Their
source registers stay live through allocation, and their position among all
stores is preserved. A different member base ends the run without consuming
the next run's leading value definition. Interior entries, relocations,
deferred fixups, and opaque instructions reject the region. Sharing reduces
value definitions while preserving the order and multiplicity of stores.
Version-specific schedules select when each value becomes ready, and ordinary
virtual-register liveness controls placement within the surrounding function.
Canaries 2068–2071 cover guarded regions; 2104–2113 exercise inline composition,
volatile stores, callbacks, and pointer rebinding. Canaries 2128–2132 extend
that boundary to independent stores between repeated interior addresses,
including ordered writes, aliasing, rebinding, and scheduling disabled.

## Dense saves for normalized array cursors

At O3+, a register-only normalized global-array cursor loop selects the dense
frame owner once five GPR homes survive calls (at most the ABI's eighteen saved
GPRs). Automatic arrays/aggregates, addressable scalar frames, and saved floating
homes retain their existing frame owners. Legacy cursor frames reserve at least
eight bytes per saved GPR, rounded to 16 bytes. This is a layout minimum rather
than a rewrite of already placed automatic storage.

Cursor save mode is a resolved version policy. An omitted `-use_lmw_stmw`
selects inline multiple-register instructions on linkage-first builds and helper
calls on newer builds. An explicit override wins. The existing boolean remains
available to older owners; the explicit bit lets a measured owner distinguish
default from `off`. Helper saves precede anchor initialization, preserving the
incoming saved-register image before any new value can occupy it. Canaries
2114–2120 vary live cursor count, parameter/local survivors, optimization, debug,
scheduling, and both explicit save modes.

## BSS-base lifetime after cursor induction

The writable-section planner evaluates modern cursor loops after the same
source induction rewrite used by body lowering. References moved into the
loop initializer do not themselves make the section base live across the
loop's callbacks. Later references to that BSS section can require a persistent
base; the existing path-sensitive reference/call analysis decides this. The
modern path requires O3+, a successfully normalized cursor loop, and qualifying
full-BSS references. Existing legacy writable-section selection remains separate.
Small-data and initialized-data objects never contribute to this BSS base.

Section-relative addresses remain deferred until canonical unit layout is
known. Frame-prefix rotations and entry-argument moves must therefore carry
deferred displacements as well as relocations and branch targets. Use the
shared machine-function remapper; relocating only ELF fixups is insufficient.
Canaries 2121–2127 cover post-loop lifetimes, leading callbacks, mixed sections,
and 32-KiB/64-KiB displacement boundaries across optimization and save modes.

## Member-value lifetimes through guarded stores

A retained member constant may extend into the fallthrough arm of an immediately
following comparison and branch. Recognize one repeated literal followed only
by ordinary byte/halfword/word stores, ending at the join or at the branch around
an else arm. Every entry into the condition and store arm must pass through the
preceding member-value graph. A live scratch on exit, metadata-owned instructions,
calls, reads, and different literals prevent this extension.

The same virtual value feeds the original member stores and the guarded stores.
Ordinary allocation handles its lifetime across the condition, including
interference with arguments and values used in the other arm. Remove only the
repeated literal, using the shared instruction editor to preserve branch and
fixup ownership. Canaries 2133–2137 cover both arm layouts, nonzero constants,
volatile store order, aliasing, bypass entries, and barriers.

## Leaf guards after store prefixes

A leading store run followed by a terminal guard can require the shared CFG
lowerer even when the guard's arm contains only one semantic statement. Chained
assignments count as one statement while emitting several ordered stores.
Admit that shape after dedicated store owners decline it, using the existing
leaf eligibility checks, named-value planning, and return handling. A callback
is not required to obtain correct CFG lowering. Canaries 2138–2142 cover simple
and chained guarded stores, nonzero values, volatile writes, aliasing, nested
conditions, intervening loads, and a result computed after the stores.

## Prefix literals retained by fixed fills

A fixed-trip fill can share a literal already used by a straight-line prefix.
Prove the scratch definition dominates the loop setup and that no intervening
instruction redefines it. Require the original scratch to be dead after the
loop, and reject alternate entries or symbolic ownership of the literal.
Promote the prefix definition and consumers to a virtual value that also feeds
the expanded stores; allocation handles overlap with address setup and CTR
initialization. This removes the loop's repeated literal without moving prefix
memory operations. Existing version-dependent expansion factors and exit-value
rules still apply. Canaries 2143–2148 cover scalar/global prefixes, ordered
writes, aliasing, remainder packets, returned cursors, and barriers.

## Shared parameter images in unoptimized loops

GC/1.1p1 O0 can assign every single-use word parameter to SP+8, including
parameters whose lifetimes overlap and slots occupied by saved GPRs. Preserve
this under the existing version policy. Count syntactic source uses, without
loop-frequency weighting. Assign retained locals in first-definition order,
then retained parameters in declaration order, to descending saved registers.
Write parameter images before evaluating local initializers: an initializer
can therefore reload a later parameter's overwritten image.

Keep this source-home plan separate from CFG emission. The shared loop lowerer
has an O0 entry-test policy; ordinary optimized callers retain their existing
first-iteration proof. Frame helpers and individual/multiple saves use the
existing frame emitters. Shared statement lowering handles the loops and
callbacks. A narrowly checked literal-before-spilled-pointer-load reorder
preserves the reference's register state even when the next store faults.
Instruction edits retain branch and symbolic metadata ownership.

This owner accepts call-bearing void loops with word/pointer parameters and
locals, and rejects address-taking, local arrays, assembly, existing frame
slots/data anchors, and unsupported statements. Other profiles retain their
existing owners. Canaries 2149–2153 exercise parameter aliases, copied cursors,
source-use counts, local ordering, scalar images, odd/even frames, and explicit
save modes; original corruption and faults are part of their reference model.
