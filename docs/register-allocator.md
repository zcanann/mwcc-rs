# Phase D — the register allocator (the keystone)

This is the design for the subsystem the whole project hinges on. Everything
below the level of single-expression functions — real reference objects, the
operand/base/const register flows mwcc threads through `r3`, `r4`, … — is gated
on reproducing mwcc's **register allocation**. This document records what we have
learned about that allocator empirically (every rule is backed by oracle diffs),
the IR we will introduce to give those decisions a home, and a migration that
keeps all builds byte-exact at every step.

## The problem we are solving

Today, instruction selection chooses registers **inline** as it walks the AST.
There is no separate allocation pass, so there is nowhere for an allocator to
*live*. The current model gets a long way (484 reloc-exact canaries across 8
builds) because it encodes mwcc's allocation behavior as a set of local rules at
the point of emission — the "anchor model," the scratch conventions, the
free-register pickers in `placement.rs`. But those rules are per-pattern, and the
cases that defer (`two globals as operands`, `f(p->a, p->b)` call-arg base
preservation, the comparison-to-bool idioms over non-leaf operands, incremental
in-register local mutation) all defer for the *same* reason: they need a real
allocator that reasons about live ranges across a whole function, not one binary
node at a time.

## What we know about mwcc's allocator (empirical, oracle-backed)

These are the invariants the current code already relies on, plus the rules the
deferred cases need. They are the allocator's specification.

### Registers and conventions
- **GPRs**: `r3`–`r12` are the freely-allocatable pool. `r3`–`r4`/`r1` are
  argument/return/stack per EABI; allocation favors the lowest free register.
- **`r0` is special and is never a base or a live-value home.** `addi rD, r0, x`
  and `lwz rD, x(r0)` read *literal zero*, not the register — the `li`/literal
  trap. So `r0` is only ever a transient scratch for a value that is consumed
  immediately. This single fact explains the absolute-addressing fold rule (see
  below) and why the allocator must keep `r0` out of any computed address.
- **FPRs**: `f1`–`f13` allocatable; `f0` is the float scratch.

### The anchor model (operand placement)
For a binary node, one operand is the **anchor**, kept in a stable register; the
other goes to the scratch (`r0`/`f0`). The anchor is the **left** operand for a
commutative operator and the **right** for subtraction (so `subf` computes
`left - right`). A located operand (`*p`, `p->f`, a global) anchors in its own
address/base register; a leaf anchors in its home register.

### Free-register selection
The lowest GPR in `r3..=r12` that is neither the scratch nor reserved (nor, where
relevant, a named exclusion). "Reserved" = the registers holding values that must
survive the evaluation of a sibling sub-expression. This is the seed of live-range
tracking: the allocator generalizes `reserved` to real liveness.

### Absolute-addressing base coalescing (`-sdata 0`)
Materializing a global's address needs a base GPR (`lis base, sym@ha`). Because
`r0` can never be a base:
- A **load into a non-`r0` GPR** uses that register as the base and **coalesces**
  (no fold): `lis dest; addi dest,dest,sym@l; lwz dest,0(dest)`.
- A **load into the scratch `r0`**, a **float load** (FPR dest), or a **store**
  takes a *separate* lowest-free base and **folds** `@l` into the memop:
  `lis base; lwz r0,sym@l(base)`. The base avoids the sibling operand (kept live).
- A store materializes the base **before** the value (mwcc's schedule).

This base-vs-value coincidence is an *allocation outcome*. The current code models
it directly; under the vreg allocator it should fall out of coalescing + the `r0`
constraint, not a special case.

### Narrow operands batch loads ahead of extensions
Two signed-`char` globals in `a + b` emit `lbz; lbz; extsb; extsb; add` — both
loads, then both sign-extensions — not interleaved. The allocator/scheduler
groups the memory ops and then the fixups. (Signedness is build-dependent:
`Behavior.char_is_signed`; build 53 omits the `extsb`.)

### The deferred cases (what the allocator must additionally do)
- **Two located operands as a binary node** (two globals, two members of the same
  base): both need a stable register simultaneously; today only one anchor fits.
- **Call-argument base preservation**: `f(p->a, p->b)` needs `p` copied
  (`mr r4,r3`) before `r3` is overwritten by the first load.
- **Comparison-to-bool over non-leaf operands** (`p->a > x`): mwcc reallocates the
  whole branchless idiom, operands landing in fresh temps.
- **Incremental in-register local mutation**: `t=a+b; t=t+c` → `add r3,r3,r4;
  add r3,r3,r5` — the local is allocated to a register and updated in place,
  rather than re-materialized.

## The IR: a virtual-register stream

Introduce a representation between instruction selection and machine code: the
same `Instruction` shapes but over **virtual registers** (an unbounded `VReg`
space) instead of physical numbers, plus the metadata the allocator needs.

```
AST → (selection) → VRegFunction → (allocation + scheduling) → MachineFunction → object
         emits vregs                assigns physical regs,
         + value classes,           inserts spills/extends,
         + anchor hints             fixes the schedule
```

- Selection stops choosing `r0`/`r3`/lowest-free; it emits vregs and *hints*
  (anchor side, "must be the return value," "is an address base," class
  GPR/FPR). All the knowledge currently in `placement.rs` becomes hints, not
  decisions.
- The allocator consumes vregs + hints + liveness and produces physical
  registers, honoring the invariants above (the `r0` constraint, anchor
  stability, coalescing, lowest-free). The scheduler orders within a block to
  match mwcc (e.g. address-before-value, batched narrow extensions).

`VRegFunction` becomes a first-class, inspectable pipeline stage (a new
`--emit-artifacts` dump between AST and machine code), satisfying "clean pipeline
control of all stages."

## Migration (keep every build byte-exact at each step)

The risk is a big-bang rewrite. Avoid it:

1. **Land the IR types** (`mwcc-vreg` crate) with the allocator interface and unit
   tests, unwired. No behavior change. — **DONE.** `Class`/`VirtualRegister`/`Reg`,
   `RegisterConstraints` (the pools + the `r0`-never-a-base rule as data), and
   `Allocator`/`LinearScan` over live intervals with pinned occupancies, 11 tests.
   First integration: the generator's free-register helpers now draw their pools
   from `RegisterConstraints` (one authoritative home, shared with the allocator),
   still byte-exact across all 8 builds.
2. **Wire the pass, no fork.** — **DONE.** `lower_function` runs `analyze ->
   LinearScan -> apply` on every function. Selection still emits physical
   registers by default, so with no virtuals the pass is a no-op — one pipeline,
   not a legacy/vreg fork. A migrated site just emits a fresh virtual and the pass
   resolves it. The machine description (`for_each_register`, all 75 variants) and
   precise per-definition liveness with half-open interference (a result reuses a
   source that dies at its definition) reproduce the inline allocator's choices.
   Virtuals ride in the existing `Instruction`'s u8 fields via the `VIRTUAL_BASE`
   convention (transitional; parameterize `Instruction<Reg>` if a function needs
   >224 virtuals/class).
3. **Migrate sites one at a time, byte-exact.** — **IN PROGRESS.** First site:
   `place_general_operands`' both-complex temporary (`(a+b)*(c+d)`) — the allocator
   reproduces it exactly (temp -> r3 / r5 as the inline code chose). Then the
   deferrals the allocator *removes*, each byte-exact for its core shape, with new
   canaries: two-global (`(g+h)*x`), two-dereference (`(*p+*q)*x`), the
   add-into-scratch trap (`((a*b)+1)*c` — the marioparty4 `rand.c` blocker),
   two-float-load (`(*p+*q)*z`, the first FPR-class migration), and two-member
   (`(p->a+p->b)*x`). The migration recipe at each site: replace a
   `lowest_free_general()` / `free_register_avoiding()` / required-non-scratch-
   destination with `fresh_virtual_general()` (or `fresh_virtual_float()`); the
   pass coalesces it onto the register the inline code chose. A side effect worth
   noting: the `destination` parameter is now gone from the whole placement chain
   — register choice left placement entirely. Where a removed deferral exposes an
   optimizer (reassociation `(g+h)+x`) or scheduler (operand order `(*p+2.0f)*z`)
   difference, the site is **kept deferring** rather than emit correct-but-non-
   matching bytes — that is a Phase E concern, separated cleanly.
4. **Phase E — the scheduler — is started and functional** (`schedule.rs`). mwcc
   reorders within a block for the Gekko's in-order dual-issue pipeline, hoisting
   long-latency multiplies/divides ahead of cheap ops. The pass is a data-
   dependence DAG + list scheduling with a latency-rank policy, run **before
   allocation on the virtual-register stream** so physical-register reuse can't
   fabricate false dependencies that block a legal hoist; allocation then colors
   the scheduled order. It fixes `((a*b)+1)*(c*d)`, `(a+b)*((c*d)+1)`, and
   divide+multiply ordering byte-exact. v1 skips functions with a forward branch
   (the branch's index target would need remapping). The remaining misses are now
   *allocation*, not order: two intermediates forced into the scratch `r0`
   fabricate a false dep that blocks a hoist — the fix is the deeper allocator-
   scheduler co-design (migrate the last `r0` intermediates to virtuals and let
   the allocator assign `r0` as a transient, matching mwcc's own r0-vs-real
   choice per context).

5. **Still ahead:** that scratch/`r0` co-design; generalizing coalescing so the
   anchor model / ABS base coalescing / narrow batching are *derived* not hard-
   coded; the call-argument base-preservation and comparison-to-bool deferrals;
   forward-branch target remapping to lift the scheduler's straight-line limit.

The contract never changes: byte-exact or an honest deferral — never wrong bytes.

## Why this unlocks real objects

Real reference objects (marioparty4 `rand.c`, the DOL/REL units) thread
intermediates through real registers and hit every deferred case above. With the
allocator in place — plus the addressing modes (done: SDA21 + ADDR16) and a
fuller frontend (Phase F) — the project's north star becomes reachable:
byte-identical output for whole translation units. The allocator is the single
highest-leverage subsystem between here and there.

## The __va_arg policy specification (fires 645-654, canaries 1154-1159)

The narrow-guard construct campaign measured the allocation/scheduling policies a
general multi-local path must reproduce. Each is oracle-backed; the canaries are the
acceptance tests. Insertion points named against the current crate:

1. **Consumer-tree home coloring.** A local's home is the register where the RETURN
   expression's evaluation wants it: `a+b` -> [r3, r0] (in-place add); `a+b+c`
   reassociates `a+(b+c)` -> [r4, r0, r3]. Insert as a coloring PREFERENCE on the
   locals' LiveIntervals (a preferred-register field beside `avoiding`), derived by
   walking the consumer tree the way evaluate_tail lowers it.
2. **Dying-register reclaim is KIND-dependent.** A LOAD init may take the condition
   parameter's register once the width-op copies it out (`lbz r3,0(p)` after
   `clrlwi r0,t,24`); a CONST `li` init may NOT (it takes the next volatile, r4).
   Encode as: load-defined intervals may start at the width-op's index; const-defined
   intervals start at the compare (conservative overlap with the parameter).
3. **Scratch double-duty.** r0 hosts the width-op result until the compare consumes
   it, then becomes a local's home. The interval for the test scratch must END at the
   compare, not the branch.
4. **Conflict avoidance vs record tests.** An arm containing a record-form test
   (`clrlwi.`) claims r0 inside the arm — locals live across the arm avoid r0
   (measured: b -> r5). This is a pinned occupancy across the arm's range.
5. **Live-param home shifting.** When the condition parameter is re-read by LATER
   tests (chained blocks), locals shift past it (r4, r5) and the join computes into
   r3 (`add r3,r4,r5`); when it dies at the single test, the consumer tree owns the
   homes. Dead EXTRA params' registers ARE reclaimed (unused u in r4 -> a takes r4).
6. **The latency-slot scheduler** (schedule.rs): the pending init/assign pool fills
   (a) the width-op -> compare gap (first init), (b) post-compare slots (later inits),
   (c) a record-test -> branch gap (an arm const), (d) a load -> use gap (the SPLIT
   extsb schedules after the next ready init). Generalize as list scheduling within
   the block with the measured latency ranks, run before allocation.
7. **Self-op init folds.** `x = x OP const` on a still-statically-known local folds
   to a constant (value tracking against the init, invalidated by any prior
   conditional assign).
8. **Member-address locals alias or copy.** `T* q = &base->f` aliases base's register
   (offset-0: no instruction; the reassign mutates in place) when base is otherwise
   dead; when base stays live (later member accesses), q takes a `mr` copy.

Milestone: route the canary-1154..1159 shapes through the general path (virtual homes
+ LinearScan with 1-5 + the scheduler with 6) and delete the bespoke handlers when the
oracle stays green; then attempt __va_arg (9 corpus copies), then ansi_fp/scanf.
