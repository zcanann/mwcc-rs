# Classification of differences from mwcceppc

A taxonomy of how `mwcc-rs` output differs from the real compiler, built by
running real expression shapes pulled from the decomp `reference_projects/`
(fzerox, marioparty4, animal_crossing, …) through both compilers. The headline:
**most real arithmetic already matches byte-for-byte** — `x - x*(s1+s2)`,
`x*pio2_hi + x*pio2_lo`, `a - b*quotient`, `base + i*0x80`, `arg1*vdata->angle_mult`
all match. The gaps fall into three categories, in priority order for matching
whole real objects.

## Category A — frontend parse gaps (we can't parse it)

The dominant blocker for real code. The function never reaches codegen.

- **Loops** — `for`, `while`, `do/while`. Real functions are *full* of these;
  this is the single highest-impact gap. (Caveat: mwcc unrolls/`ctr`-loops at
  `-O4`, so loop *codegen* is itself nontrivial to match — but parsing is the
  precondition.)
- **Multiple declarators** (`int a, b;`), `signed char`, `switch`, `goto`,
  compound assignment in more positions.
- **C++** — classes, methods, templates, mangling. The majority of real files.

Highest value, largest effort. This is "Phase F — frontend completeness."

## Category B — codegen defers (parsed, honest error)

Reached codegen but hit an unimplemented shape and deferred. Tractable feature
extensions — each a bounded, oracle-verifiable piece.

- **Nested float operands** — `x + v*(S1 + z*r)`: a float sub-expression as an
  operand of an FMA/arithmetic node. ("a float leaf operand must be a variable".)
- **Comparison-to-bool over non-leaf operands** — `p->a > x`: the branchless
  signed-compare idioms when an operand is a load, not a leaf.
- **Magic-number division** — `x / 7` (non-power-of-two constant divisor).
- Multi-member call arguments, and the other allocator-adjacent deferrals.

Medium value, low-to-medium effort. The clearest place to keep widening coverage.

## Category C — correct-but-non-matching (valid but different bytes)

We compute the same value in a different, equally-correct form. **Rarer in real
code than the synthetic probes suggested** — most real arithmetic is shallow
enough to match.

- **Sethi-Ullman evaluation order** on asymmetric trees (single-level integer
  case now handled; full recursive + float order outstanding).
- **Add-chain reassociation** — `(a+b)+c -> a+(b+c)`, with an `mr` to preserve
  the first operand.
- **Float instruction scheduling** — mwcc emits a block's adds before its
  multiplies (leaves-first); our latency-rank scheduler is only a proxy.
- **Scratch/`r0` collisions** — two intermediates forced into `r0` block a legal
  hoist; needs the recursive SU register/scratch scheme.

Lower value for real-object matching (rare), high research depth. The
allocator-scheduler co-design / recursive Sethi-Ullman.

## Takeaway

Order of value for matching whole real objects: **A (frontend, esp. loops) > B
(codegen defers) > C (optimizer/scheduler subtleties)**. The deep arithmetic
ordering work, while interesting, addresses the rarest category. Widen B and
start A (loop parsing, then its codegen) to move the real needle.
