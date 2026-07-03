# The e_fmod whole-function register map (fire 435)

Derived by hand from docs/efmod-knit-target.dis (the real
__ieee754_fmod, marioparty4 @ 2.6, 207 instructions). This is the
fixture the knit allocator must reproduce; anomalies vs the synthetic
captures are flagged.

## The value table

| value                | def site                  | home | dies / notes |
|----------------------|----------------------------|------|--------------|
| sx                   | 0x1c clrrwi r0,r6,31       | r0   | 0x324 (or) — **owns r0 for the whole function**; every Zero-return rlwinm reads it |
| hx-raw (lwz 8)       | 0x10                       | r6   | 0x28 (xor) |
| hy-raw (lwz 16)      | 0x0c                       | r10  | 0x18 (clrlwi) |
| lx                   | 0x20 lwz r4,12             | r4   | to the end (loop doublings keep it in place; 0x2c0/0x328 stw) |
| ly                   | 0x14 lwz r5,20             | r5   | 0x250 (fixup subf) — then r5 is REUSED twice (fixup lz, norm bound) |
| hy-abs (hy&0x7fff…)  | 0x18 clrlwi r8,r10,1       | r8   | 0x154 align — **fresh home, NOT in place** (see anomaly 1) |
| hx-abs (hx^sx)       | 0x28 xor r7,r6,r0          | r7   | 0x178 align slw / 0x15c clrlwi |
| purge lis 0x7ff0     | 0x30 lis r6,32752          | r6   | 0x4c — takes hx-raw's freed home; r0 busy (sx) |
| purge temp chain     | 0x24 or. r3 / 0x3c-0x48    | r3   | condition-only; r3 free pre-return |
| ix                   | 0xb4/0xd4 li r11 / 0xf4    | r11  | 0x1dc (subf.) — next-free past r10 |
| ilogb-x loop i       | 0xb0 mr r3,r4 / 0xd0 slwi  | r3   | arm-local; the mr aliases lx |
| iy                   | 0x110/0x130 li r3 / 0x150  | r3   | 0x2ac+ (writeback) — r3 free again |
| ilogb-y loop i       | 0x10c mr r6,r5 / 0x12c     | r6   | arm-local; hx-raw's freed home |
| post-align hx        | 0x160 oris r9 / 0x184 or   | r9   | the align r0-JOIN went to r9 here (see anomaly 2) |
| post-align hy        | 0x1a4 oris r7 / 0x1c8 or   | r7   | REUSES hx-abs's freed home |
| align-x n            | 0x168 subfic r9,r11,-1022  | r9   | in-place-ish: lands in the JOIN home pre-join |
| align-x 32-n temp    | 0x174 subfic r6            | r6   | scratch-like but r6 (r0 owned by sx) |
| loop count n         | 0x1dc subf. r6,r3,r11      | r6   | fused record + mtctr + beq (no cmpwi) |
| loop hz              | 0x1ec subf r8,r7,r9        | r8   | hy-abs's freed home (count home r6 NOT free — n dies at mtctr but r6 was reused for it… hz skips to r8) |
| loop lz              | 0x1f0 subf r10,r5,r4       | r10  | hy-raw's freed home |
| loop carry temp      | 0x204 srwi r6,r4,31        | r6   | freed count home |
| fixup hz             | 0x24c subf r6,r7,r9        | r6   | |
| fixup lz             | 0x250 subf r5,r5,r4        | r5   | ly's home, freed AT its own subf |
| norm bound 0x100000  | 0x288 lis r5,16            | r5   | r0 owned by sx → **bound NOT in r0** (contrast fire 424) |
| norm carry temp      | 0x290 srwi r6,r4,31        | r6   | |
| wb hx-HI_BIT         | 0x2b8 addis r5,r9,-16      | r5   | |
| wb (iy+1023)<<20     | 0x2b4/0x2bc addi/slwi r3   | r3   | in place (iy) |
| wb hx-word           | 0x2c8 or r0,r3,r0          | r0   | overwrites sx AT its last read |
| sub-out n            | 0x2d4 subfic r6,r3,-1022   | r6   | |
| sub-out temps        | r3/r4/r5/r9                |      | mr r9,r0 = hx=sx (r0 read, not moved) |

## Anomalies vs the synthetic captures

1. **Local rewrites take FRESH homes; param rewrites are in-place.**
   Fire 425's `hx &= 0x7fffffff` (a PARAM) folded in place
   (`clrlwi r3,r3`). Here `hy &= 0x7fffffff` (a LOCAL loaded from a
   slot) goes r10 -> r8, and `hx ^= sx` goes r6 -> r7. The loaded raw
   value and the rewritten value are SEPARATE allocator values (the
   int_alloc Load* classes), and the rewrite allocates next-free.
   Corollary: the load registers (r10, r6) become the freed-home pool
   that later segments consume (lz -> r10, purge-lis/count/carry -> r6).

2. **The diamond r0-join is CONDITIONAL on r0 being free.** Fire 431's
   align converged in r0; here sx owns r0, so align-x joins in r9 and
   align-y joins in r7 (the freed hx-abs home). The join register is
   "the rewritten value's own home", allocated per anomaly 1 — r0 was
   just what that resolved to in the synthetic capture.

3. **The ilogb loop's `i = lx` init emits `mr r3,r4`** (an explicit
   alias copy) because lx must SURVIVE the loop — contrast the
   standalone fire-413 rule "aliases rename in place". In-place
   renaming applies only when the source dies at the alias.

4. **Freed-home priority over next-free is range-based, not stack-
   based**: hz takes r8 (hy-abs died at align) not r6 (count died at
   mtctr a few instructions earlier) — wait: r6 was ALSO taken (count
   n def 0x1dc lives to mtctr 0x1e0; hz defs at 0x1ec after r6 frees).
   hz chose r8 over r6. Both free at def. Hypothesis: values freed
   LONGER ago rank first (LRU), or the pool orders by register number
   descending among recently-freed… r8 < r10 chosen for hz, r10 for
   lz (def order ascending gets ascending homes from the freed set
   {r6,r8,r10}: hz->r8, lz->r10, and r6 goes to the carry temp INSIDE
   the loop). The carry temp needing a body-local register may force
   hz/lz OFF r6 (the body reuses r6 every iteration). I.e. allocation
   is liveness-correct across the back edge: hz/lz live across the
   body where r6 is clobbered -> they avoid r6. **The freed-home pool
   is just lowest-free-at-def with whole-range conflict checking** —
   consistent with int_alloc's assign() if the carry temp is placed
   first or ranges are honored. Verify in the fitting pass.

## Fitting plan

Encode each value above as an int_alloc-style (class, def, last)
fixture over the real instruction indices, extend the model with:
first-claim r0 ownership (sx-style values), the fresh-home rewrite
rule (anomaly 1), and back-edge-aware ranges (loop values live over
the whole body). Then assert model_order+assign reproduce THIS map
before writing the knit emitter.
