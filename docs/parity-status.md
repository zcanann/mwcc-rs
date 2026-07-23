# Reference-project parity status

Last fresh holdout: 2026-07-23 02:22 UTC at compiler commit `10024016`

Latest paired checkpoint: 2026-07-23 11:08 UTC at compiler commit `93db2a25`

Latest compiler + harness fingerprint: `45858fe7bf77d8317ab08799621492b686ccf4f7ac10df82305e4a05fab0151d:41b9a774f824876f08a74f4aa39e21163e3472591d69e9c3cd74b778b87a89ab`

This file records a measurement checkpoint, not a claim that the numbers stay
current after compiler or harness changes. Canary pass counts and work-queue
counts are deliberately absent: neither is a corpus parity estimate.

## Completion proof

The configured corpus contains 47,879 translation units across 13 MWCC-configured
projects. All 13 compiler identities in the corpus are recognized. No project
matrix is complete yet. The latest fingerprint directly observed 414 configured
TUs and proved 58 whole-object exact: 40 statistical sample rows plus 18 breadth
sentinels. Old-fingerprint exact observations are not counted as proof about the
current compiler, so the literal current-fingerprint proof is:

| Measure | Result |
| --- | ---: |
| Configured TUs proven whole-object exact | 58 / 47,879 |
| Project matrices proven complete | 0 / 13 |
| Directly observed configurations at this fingerprint | 414 / 47,879 |

`fzerox` is the fourteenth discovered project but currently has no MWCC configure
metadata, so it is outside the 47,879-TU denominator. GC/1.3.2r is intentionally
not a required parity identity.

## Baseline: fresh current-population holdout

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

The exact-within-protocol share is 9.1%, with a finite-population 95% confidence
interval of 6.6%-12.4%. This remains the last unbiased population estimate. It
is also a confirmed lower bound on intrinsic eventual parity. If every unknown
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

Compiler commit `93db2a25` was run over the exact same 384 statistical rows and
30 breadth sentinels. Because this panel's membership was known during compiler
work, it measures movement on the frozen panel; it is not a new unbiased
current-population estimate.

| Whole-object outcome | Baseline | Current | Change |
| --- | ---: | ---: | ---: |
| Exact | 35 / 384 | 40 / 384 | +5 |
| Confirmed non-parity (`DIFF` or compiler `DEFER`) | 195 / 384 | 183 / 384 | -12 |
| Measurement unknown | 154 / 384 | 161 / 384 | +7 |

The current panel's exact share is 10.4%. Its descriptive finite-population 95%
interval is 7.7%-13.9%, and its exact-or-unknown identification range is
10.4%-52.3%. These describe the tuned panel and do not supersede the fresh
holdout's 9.1% current-population estimate. Among the 223 rows with authoritative,
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
  --audit-epoch 2026-07-22-status-1 \
  --audit-purpose paired-panel \
  --jobs 14 \
  --timeout 300 \
  --reference-root /path/to/reference_projects
```

Results are keyed by the compiler+harness fingerprint, so running this command
after a compiler or harness change creates a different checkpoint rather than
silently mixing observations.
