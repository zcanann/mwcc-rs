# The scheduler-model dataset

Ground-truth instruction orders from the real mwcceppc (GC/2.6, -O4,p) for
minimal scheduling shapes — the corpus the linearizer model is fit against.

- `probes.tsv`: tab-separated `name <TAB> C source` (the function must be `f`).
- `<name>.expected`: the captured order (regenerate: `tools/sched_probe.sh`).

Findings so far (fires 275-276):
- `-O4` vs `-O4,p` byte-identical on all shapes: the interleave is core -O4
  selection, not a separable pass.
- Linearization is a function of the block DAG, independent of source
  statement order for independent statements.
- Long-latency ops (divw, mulli, fdiv) schedule first; their results' stores
  wait out the latency.
- Stores keep source order ONLY when they may alias (pointer stores);
  distinct-global stores reorder when readiness/staging favors it
  (three_deep_vs_shallow, divide_chain).
- Store values stage through r0 when singly consumed; the shared staging
  register serializes chains (two_load_stores).
- Allocation is coupled to linearization (registers assigned at issue,
  deaths tracked): the same statement pair schedules differently when a
  dying register can be reused (frexp tail vs tail_pair).
