# Reference-project parity status

Last fresh holdout: 2026-07-23 22:53 UTC at compiler commit `c0962f28`

Latest paired checkpoint: 2026-07-23 17:44 UTC at compiler commit `869596ad`

Latest targeted checkpoint: 2026-09-06, legacy aggregate leaf frames (fingerprint below)

Latest measured compiler + harness fingerprint: `af32970857558c0da7ea181b727f37e9122a1ed82f13d6cb51ce35824167549b:5e4ca1ddc460f4d86cd15e9e7a834f5b2a572a7e0c80e09279a629da4eac0806`

This file records a measurement checkpoint, not a claim that the numbers stay
current after compiler or harness changes. Canary pass counts and work-queue
counts are deliberately absent: neither is a corpus parity estimate.

## Focused legacy leaf-frame follow-up, 2026-09-06

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
