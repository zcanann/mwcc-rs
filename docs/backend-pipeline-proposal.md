# Backend pipeline proposal

Status: proposal (2026-09-28). Nothing here is implemented yet.

## Why

The front end (loader, lexer, parser), the object writer, and the per-build
`CodegenProfile`/`Behavior` system are sound. The backend is not a pipeline:

| Crate | Lines |
| --- | ---: |
| `mwcc-syntax-trees-to-machine-code` | 687,347 (1,658 files, 1,199 `try_*` owners) |
| `mwcc-tokens-to-syntax-trees` | 45,748 |
| `mwcc-syntax-trees-to-debug-info` | 11,070 |
| `mwcc-vreg` (allocator, scheduler) | 10,146 |

Each `try_*` owner recognizes a source shape and emits a finished,
hand-scheduled instruction stream. That generalizes poorly:

- **Canaries vs corpus.** Canaries are ~65% whole-object exact per build; a
  1,000-row random sample of configured reference TUs (`win_parity.py
  --sample 1000 --seed 101`) is 69/852 TUs (8.1%) and 1,969/17,969 functions
  (11.0%) exact.
- **Flat blocker histogram.** Across that sample no single per-function skip
  reason exceeds ~50 functions. Four targeted fixes landed on 2026-09-28
  (LR-gap scheduling, leaf/constant/global argument order, narrow argument
  conversion, direct-init locals) each verified against the real compiler,
  yet the sample's exact-function count did not move: every function that
  passed one owner met the next missing shape.
- **Structural gaps no owner can close.**
  - *Loop transformations.* MWCC unrolls counted loops at `-O4`
    (`dvderror.c`'s 18-entry search: 9× body, `ctr`=2, `lwzu` strength
    reduction). There is no loop representation to transform.
  - *Line tables.* Instructions carry no statement provenance, so `.line`
    falls back to open/close-brace rows. `-sym on` covers ~60% of the corpus
    (AC, TP, WW, OoT); none of those objects can match.
  - *Allocation/scheduling correctness.* Owners allocate and schedule
    locally. Example: `k2(b-1, a)` emitted `mr r4,r3; addi r3,r4,-1`
    (clobbers `b`) — MWCC resolves the parallel move through r0.
- **Compile time.** Large C++ units still take 10–90 s (parser probes; see
  `tools/win_sample.py`).

## MWCC's own stage order

The CC0 decompilation of GC/1.2.5 (github.com/JackPriceBurns/mwcc,
`docs/PASS_PIPELINE.md`) recovers the real sequence from trace strings:

- Frontend `IRO_Optimizer` on expression trees: build flow graph, evaluate
  conditionals, remove unreachable/redundant jumps, copy/constant propagation,
  range propagation, expression propagation, use-def, constant folding,
  `IRO_LoopUnroller` (innermost loops; rejects induction-used-in-loop and
  multiple exits), `IRO_FindLoops`, a second propagation/folding round,
  common subexpressions, jump chaining.
- `CodeGen_Generator` stages: `INITIAL CODE` (lowering to PCode over virtual
  registers) → backend PCode optimizer → `AFTER INSTRUCTION SCHEDULING` →
  `AFTER PEEPHOLE FORWARD` → `AFTER REGISTER COLORING` (interference-graph
  coloring with spill-and-retry) → `AFTER GENERATING EPILOGUE, PROLOGUE` →
  epilogue/prologue merge → table-driven backward peephole with liveness DCE →
  `FINAL CODE AFTER INSTRUCTION SCHEDULING`.

Scheduling therefore runs on virtual registers *before* coloring, and again on
physical code at the end. The new stages below mirror these boundaries.

## Target shape

MWCC itself is a conventional compiler: front end → per-function IR →
optimizer → instruction selection → coloring allocator → list scheduler →
peephole. Byte parity is most reachable by mirroring those phases, each
parameterized by the existing build profile.

Following the repository's own rule (representations are nouns, pipeline
crates are `x-to-y` transforms) and the numbered-stage layout used by Omega:

```
representations/
  mwcc-syntax-trees      (existing) source-shaped syntax trees
  mwcc-iro               typed IRO-level IR ~ MWCC's ENode trees: every node
                         typed, memory as base + index + offset loads/stores,
                         pointer arithmetic scaled, one return value
  mwcc-pcode             basic blocks of mwcc-machine-code instructions over
                         virtual registers, per-instruction source provenance
  mwcc-machine-code      (existing) physical instructions
pipeline/
  mwcc-syntax-trees-to-iro            build the typed IR; IRO passes (folding,
                                      algebra, sign idioms, select rewriting,
                                      store narrowing), each switchable
                                      (MWCC_IRO_NO_<PASS>)
  mwcc-iro-to-pcode                   INITIAL CODE: instruction selection only
  mwcc-pcode-to-machine-code          copy propagation, webs, virtual
                                      scheduling, coloring, frame layout,
                                      prologue/epilogue, final scheduling
  mwcc-pcode-path                     drives the stages; installed into the
                                      legacy backend's hook by the mwcc driver
                                      (so PCode edits never rebuild legacy)
  (existing) machine-code-to-object, syntax-trees-to-debug-info
             — the latter reads provenance from PCode instead of guessing
```

Every stage rejects unsupported input with a diagnostic at its own boundary
(the project's "fail honestly" rule).

## Migration: strangle, don't rewrite

1. The driver tries the new pipeline per function. If any stage rejects, the
   function falls back to the legacy owners unchanged.
2. A function family moves to the new pipeline only when the gate holds:
   - canary oracle (`tools/win_oracle.sh`, all builds): no PASS lost;
   - benchmark (`win_parity.py --sample 1000 --seed 101`): exact-function
     count does not drop, per build.
3. Legacy owners are deleted once no function routes to them (trace with
   `MWCC_TRACE_OWNER=1`).

Suggested order, by corpus weight and tractability:

1. Straight-line C functions on GC/1.3–2.7 (`-sym off` first): mid-IR,
   selection, existing allocator/scheduler.
2. Provenance-driven `.line` tables for those functions (unlocks `-sym on`).
3. Calls and argument marshaling as a real parallel move.
4. Counted loops, then MWCC's unroller.
5. GC/1.1–1.2.5n linkage-first frames; 4.x (GC/3.0a3+, Wii) optimizer deltas.
6. C++ lowering (constructors, vtables, EH tables) onto the same IR.

## Measurement

Report per milestone: canary PASS counts per build, benchmark TU/function
exactness per build, and the owner-trace share of functions handled by the
new pipeline.
