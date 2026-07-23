# Reference-project parity status

Last measured: 2026-07-23 02:22 UTC  
Compiler commit: `10024016`  
Compiler + harness fingerprint: `9163533d372182a40e24d8b768389ebbb79256b1ecde00638c93b2f6c7ec1490:41b9a774f824876f08a74f4aa39e21163e3472591d69e9c3cd74b778b87a89ab`

This file records a measurement checkpoint, not a claim that the numbers stay
current after compiler or harness changes. Canary pass counts and work-queue
counts are deliberately absent: neither is a corpus parity estimate.

## Completion proof

The configured corpus contains 47,879 translation units across 13 MWCC-configured
projects. All 13 compiler identities in the corpus are recognized. No project
matrix is complete yet. The result cache contains 92 whole-object byte-exact
observations, so the literal accumulated proof is:

| Measure | Result |
| --- | ---: |
| Configured TUs proven whole-object exact | 92 / 47,879 |
| Project matrices proven complete | 0 / 13 |
| Directly observed configurations at this fingerprint | 592 / 47,879 |

`fzerox` is the fourteenth discovered project but currently has no MWCC configure
metadata, so it is outside the 47,879-TU denominator. GC/1.3.2r is intentionally
not a required parity identity.

## Fresh current-population holdout

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
interval of 6.6%-12.4%. This is the defensible current headline. It is also a
confirmed lower bound on intrinsic eventual parity. If every unknown row were
non-exact the intrinsic share would be 9.1%; if every unknown row were exact it
would be 49.2%. That 9.1%-49.2% identification range is intentionally shown
instead of guessing through missing evidence.

Unknown attribution is 99 harness/time-budget failures, 41 missing dependencies,
and 14 invalid captured configurations. A compiler `DEFER` is not unknown: it is
confirmed non-parity. Of the 42 sample rows that emitted an object, 35 were exact
and 7 differed (83.3% conditional exactness). That conditional number is useful
for backend diagnosis but must not be presented as feature or corpus coverage.

Relocation-aware diagnostics were available for 12/384 sample objects: 28/49
reference functions were exact and 840/3,984 reference code bytes were exact.
These diagnostics do not earn whole-object parity credit.

## What the audit says to work on

The largest sampled compiler blocker families were:

| Family | Sample rows |
| --- | ---: |
| C++ types, layout, and calls | 55 |
| Other unsupported lowering | 30 |
| Backend lowering, registers, and scheduling | 29 |
| Front end, parsing, and resolution | 21 |
| Control flow | 20 |
| Data and global initialization | 15 |
| ABI and runtime semantics | 9 |
| Emitted-object mismatches | 7 |

The measurement itself took 3,226.8 seconds of active wall time. Median row time
was 3.27 seconds, while p95 and maximum were approximately 300 seconds. Large
Twilight Princess and Wind Waker translation units exhausted the 300-second cap,
accounting for most of the 99 harness unknowns and most audit wall time. Making
those units reach a precise compiler diagnostic quickly, plus repairing missing
dependencies and invalid configurations, will narrow the status interval more
than increasing the random sample size today.

## Iteration and reporting contract

- Inner-loop work draws from a failure-biased queue. Previously exact rows do not
  consume the default budget; a regression simply re-enters the queue.
- A fixed paired panel is run only at explicit checkpoints to measure movement.
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
  --audit-purpose fresh-holdout \
  --jobs 14 \
  --timeout 300 \
  --reference-root /path/to/reference_projects
```

Results are keyed by the compiler+harness fingerprint, so running this command
after a compiler or harness change creates a different checkpoint rather than
silently mixing observations.
