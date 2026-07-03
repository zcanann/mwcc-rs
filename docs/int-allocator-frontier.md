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

## THE DISCARDED-HOME ANOMALY (fire 396 discriminators)

A loaded local whose home's LAST READ is before the first branch and
whose rewrite is a fresh value ("discarded home") breaks the base
order. Discriminators (same scaffold as V1b unless noted):

| id | i1's fate                                  | temp | mask | i0 | i1 | j0 | i  | class |
|----|--------------------------------------------|------|------|----|----|----|----|-------|
| D1 | `i1 &= 0x7ff` (const-mask → clrlwi r0)     | r3   | r4   | r5 | r6 | r3 | r3 | fits  |
| D2 | `i1 = 5`                                   | r3   | r5   | r6 | r3 | r4 | r4 | V1b   |
| D3 | `i1 = 0` ordered BEFORE `i0 &= ~i`         | r3   | r5   | r6 | r3 | r4 | r4 | V1b   |
| D4 | BOTH discarded (`i0 = 0; i1 = 0`)          | r3   | r4   | r6 | r5 | r3 | r0 | NEW   |
| D5 | i1 dies in a SECOND condition (post-branch)| r3   | r4   | r5 | r6 | r3 | r3 | fits  |

Findings:
- The trigger is precisely "home dead before the first branch, then a
  fresh-value rewrite". The rewrite's VALUE (0 vs 5) and the mutation
  ORDER are irrelevant (D2/D3 ≡ V1b). A rewrite that READS the home in
  block 2 (D1) or a death after the first branch (D5) fits the base
  model.
- D1 wrinkle: a CONSTANT self-mask (`&= 0x7ff`) computes clrlwi into
  r0 and stores from r0 (the home is read, not rewritten) — unlike
  W10's variable `&= ~i` which lands in the home.
- V1b/D2/D3 (ONE discarded + a crossing i0): the discarded local is
  PROMOTED to right after the temp — verified order
  [temp, i1, j0, mask, i, i0] reproduces every register via
  lowest-free.
- D4 (BOTH discarded, nothing crosses): NO promotion — the pair goes
  at the END, in DEATH-DESC among themselves (i1 then i0):
  [temp, j0, mask, i1(r5), i0(r6)]; the single-use i drops to r0.
- The unifying key is still unknown: promotion-to-front (V1b) vs
  demotion-to-back-swapped (D4) must fall out of one rule. Every
  simple global key tried by hand fails one side: death asc/desc, def
  asc/desc, statement order, final-write order, crossers-first,
  block-locals-first, loads-def-asc-then-temps.

## NEXT: the offline fitter

Hand-fitting has stalled at 12 fixtures / 3-outlier structure — the
float campaign's answer at this exact stage was the deep-fit
enumerator. Build the analog: encode the 12 register maps as vreg
fixtures, enumerate (order key × promotion rule × range extension ×
r0 policy) against all of them simultaneously, and keep whatever
combination scores 12/12. Candidate dimensions worth encoding first:
the discarded-class handling (front/back/death-desc), whether ranges
extend to stores, and tie-breaks by def position vs frame offset.

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
