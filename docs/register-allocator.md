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
   tests, unwired. No behavior change.
2. **Lower then re-raise, identity-allocate.** Have selection emit vregs for one
   self-contained slice (start with leaf arithmetic), allocate with a trivial
   pass that reproduces *exactly* today's physical assignment for that slice, and
   diff against the oracle — must stay byte-exact. Expand the slice until all of
   selection routes through vregs, the allocator still reproducing current output.
3. **Generalize the allocator** to real liveness + coalescing, verifying the whole
   canary suite stays green as each former special case (anchor, ABS coalescing,
   narrow batching) is *derived* rather than hard-coded.
4. **Tackle the deferred cases** one at a time (two located operands, call-arg
   preservation, comparison idioms, incremental mutation), each with new canaries,
   each verified across all 8 builds.

The contract never changes: byte-exact or an honest deferral — never wrong bytes.

## Why this unlocks real objects

Real reference objects (marioparty4 `rand.c`, the DOL/REL units) thread
intermediates through real registers and hit every deferred case above. With the
allocator in place — plus the addressing modes (done: SDA21 + ADDR16) and a
fuller frontend (Phase F) — the project's north star becomes reachable:
byte-identical output for whole translation units. The allocator is the single
highest-leverage subsystem between here and there.
