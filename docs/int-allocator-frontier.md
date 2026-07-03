# The int local-allocator frontier (task #20)

Fixture bank + the working model for mwcc's local register allocation in
frame/punned shapes, derived fires 391/395. Regenerate any capture with
`tools/probe.sh '<source>' 2.6 real`.

## THE WORKING MODEL (fits 8 of 9 fixtures)

Values are assigned physical registers **in ascending order of death**
(last read, inclusive of stores), each taking the **lowest register in
r3..r10 free over the value's whole live range** (def instant included —
a value defined at another's last-read instant still conflicts).

**The r0 scratch** takes values whose range crosses **no branch**: the
guard fold, record-idiom temps, store-only rewrites (`i1 = 0`), and a
single-use shifted mask (V1's `i`, def+use adjacent). A multi-use value
crossing a `bne` (V1c's `i`) needs a real register.

**Store-only elimination** (W11, V1d): a local whose loaded value is
never read is not loaded at all; its new constant materializes in r0
(hoisted before the spill when unconditional — V1d's `li r0,0` between
lis and stfd, with its store woven right after the first load).

**Constant synthesis** (`0xfffff` → `lis rT,0x10; addi rM,rT,-1`)
claims its temp and result registers through the same model — the temp
usually lands r3 (first death), the result r4.

## THE FIXTURES

Shape family: `double f(double x)` punned int locals, guard local
`j0 = ((i0>>20)&0x7ff)-0x3ff`, unsigned shift local `i = C >> j0`.

| id  | shape                                             | temp | mask | i0 | i1 | j0 | i  | fits |
|-----|---------------------------------------------------|------|------|----|----|----|----|------|
| V1  | 2 punned, single-use i (test only)                | r3   | r4   | r5 | r6 | r3 | r0 | YES  |
| V1b | 2 punned, multi-use i, `i1 = 0` (home dies @or.)  | r3   | r5   | r6 | r3 | r4 | r4 | **NO** |
| V1c | 1 punned, multi-use i (`i0 &= ~i`)                | r3   | r4   | r5 | —  | r3 | r3 | YES  |
| V1d | V1b minus early return (no branch)                | r3   | r4   | r5 | r0*| r3 | r0 | YES  |
| W4  | 1 punned, i in TWO conditions                     | r3   | r4   | r5 | —  | r3 | r3 | YES  |
| W7  | small const (li, no temp)                         | —    | r4   | r5 | —  | r3 | r3 | YES  |
| W8  | no const (`i = i0 >> j0`)                         | —    | —    | r4 | —  | r3 | r3 | YES  |
| W10 | V1b but `i1 &= ~i` (home lives to stw)            | r3   | r4   | r5 | r6 | r3 | r3 | YES  |
| W11 | V1b but i1 NOT in test (never loaded)             | r3   | r4   | r5 | r0*| r3 | r3 | YES  |

(*) the store-only NEW value in r0; the original is never loaded.

## THE V1b ANOMALY

V1b differs from W10 only in i1's mutation (`i1 = 0` vs `i1 &= ~i`),
ending i1's home range at the or. (mid-block-1) instead of the store.
Real assignment behaves as if **i1 were assigned FIRST** — with i1=r3
pinned, everything else follows lowest-free exactly:
j0[6,7]→r4, mask[2,8]→r5, i0[4,15]→r6, i[8,13]→r4 (j0 dead).

Orders tried and falsified for V1b: death asc (the model — predicts
j0=r3), death desc, def order, original-statement order, final-write
order, crossers-first, block-locals-first. The missing ingredient is
whatever promotes a mid-death loaded-local ahead of shorter-lived
temps; find it with more discriminators (vary WHERE i1's home dies:
in the condition vs the first mutation vs a second condition).

## Emission facts (independent of the anomaly)

- `~i` with TWO consumers (W10): materialized once — `not r0,r3` then
  plain `and`s; single consumer (V1c/W7/W8): fused `andc`.
- The mask-test compare: 1 punned → `and. rScratch` (record form);
  2 punned → plain `and` + `or. r0,i1,r0` (i1 FIRST — the opposite
  operand order from the constant-mask compound).
- `(0x7ff0)>>j0` small consts: plain `li r4,K` in the same slot the
  lis/addi pair occupies (hoisted before the spill).
- W4's second condition reuses i from its home (`or. r0,r5,r3` with
  the mutated i0) — conditions do not re-materialize the mask.
