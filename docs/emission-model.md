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

- Counted + straight-line body: the x8 unroll (DEFERRED).
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
