# Floating snapshot scheduling measurements

These captures separate operation-graph identity from instruction ordering and
FPR allocation. They are diagnostics, not object-parity results.

`wind_waker_gc_1_2_5n.json` records two configured D44J01 matrix functions from
`src/dolphin/mtx/mtxvec.c`, using the original GC/1.2.5n compiler and candidate
`dc529ff5`. Object hashes identify the exact captured inputs. The candidate has
the same ordered-operand operation graphs as the reference but only 3/36 and
4/30 operations at the same positions. Both bodies remain nonexact.

Each node retains its opcode, FPR slots, direct producer indices, incoming
parameter references, and memory address/epoch. `extra_deps` preserve ordering
across potentially aliased stores. `reference_order` maps reference instruction
positions to candidate node indices. It excludes the terminal `blr`.

Generate a comparable capture from retained reference-parity objects:

```sh
python3 tools/float_snapshot_graph.py ref.direct.o our.configured.o \
  --function C_MTXMultVec --function C_MTXMultVecSR \
  --objdump target/reference-parity/tools/binutils/powerpc-eabi-objdump \
  --output target/snapshot-graphs.json
```

The tool requires stable GPR bases and a straight-line single-precision body.
It rejects unsupported instructions, including control flow, instead of
reporting partial graphs as matches. Distinct memory reads retain distinct
identities; arithmetic operand order is preserved. The graph comparison does
not prove source-level equivalence, volatile semantics, or relocation parity.

Canaries 1549 and 1550 vary row count and layout for follow-up measurements.
They currently remain nonexact across all eleven measured compiler identities.
A latency-only fit of the existing scheduler does not reproduce either matrix
schedule; changing latencies alone is not a validated lowering change.
