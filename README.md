# mwcc-rs

A byte-matching reimplementation, in Rust, of **Metrowerks CodeWarrior for Embedded PowerPC** (`mwcceppc`) — the compiler used to build GameCube/Wii games — for use in decompilation projects.

First target: **GC/1.3.2 = mwcceppc 2.4.2 build 81** (the compiler FFCC and many other titles need). The goal is to emit `.text` that is **byte-for-byte identical** to the real compiler, then expand language coverage and add more versions.

## Why

Decomp projects must reproduce the original compiler's *exact* output. The real `mwcceppc` is a closed 2002 Windows binary whose register allocator and instruction scheduler are not understood and not patchable. Some builds (e.g. an FFCC-era **1.3.1**) appear to be entirely unpreserved. An open, inspectable, *modifiable* compiler that matches `mwcceppc` would:

- let projects match functions the stock binary can't (different allocator/scheduler behavior),
- be diffable/patchable to reconstruct missing point builds,
- serve the whole GC/Wii decomp community (FFCC, BFBB, and others hit the same wall).

## Approach: A/B against the oracle

The real `mwcceppc` (run via `wibo`) is the **source of truth**. Development is a tight TDD loop:

1. Add a tiny C program to `corpus/`.
2. `harness/abtest.sh` compiles it with **both** the real compiler and `mwcc-rs`, then diffs the `.text` disassembly.
3. Make `mwcc-rs` match, byte-for-byte. Grow the corpus.

```sh
cargo build --release
./harness/abtest.sh 1.3.2      # PASS/FAIL per corpus entry, with diffs
```

The harness expects the FFCC checkout for tooling (`wibo`, the compiler set, `powerpc-eabi-objdump`); override with `FFCC=/path/to/FFCC-Decomp`.

## Status

v0 — pipeline complete (lexer → parser → codegen → ELF32 BE PPC object), **9/9 corpus byte-exact** vs GC/1.3.2:
leaf functions, integer return / args (`r3,r4,…`→`r3`), float return / args (`f1,f2,…`→`f1`),
`+ - *` int (`add`/`subf`/`mullw`), `+ - *` float (`fadds`/`fsubs`/`fmuls`), 16-bit and 32-bit constants
(`li`, `lis`+`addi` ha16/lo16), redundant-move elision.

## Roadmap (milestone ladder)

Each milestone = a corpus tier that must stay 100% byte-exact before moving on.

- **M1 — expressions & locals:** deeper expression trees, local variables, stack frame
  (prologue/epilogue: `stwu`/`mflr`/`stw`…/`lwz`/`mtlr`/`blr`), spill model, the
  **register allocator** (this is where FFCC's f2/f3 divergence lives — the core research target).
- **M2 — control flow:** `if`/`else`/`while`/`for`, comparisons, `b`/`bc` and the
  **instruction scheduler** (the `pppColum`-class reorders).
- **M3 — memory & types:** pointers, structs, arrays, `char`/`short`/`u8`…, loads/stores,
  `.data`/`.sdata`/relocations, the **float/double constant pool** (the int→float bias the
  FFCC `randchar` cast needs).
- **M4 — calls & ABI:** function calls, arg marshalling, varargs, returning aggregates.
- **M5 — C++ subset:** name mangling (Metrowerks ABI), member fns, references, `inline`,
  simple templates — enough to compile real decomp TUs like `pppRandCV.cpp`.
- **M6 — versions:** parameterize codegen by build; add GC/1.2.5n, 2.0, 2.6, 2.7, and a
  reconstructed 1.3.1. Validate against each real binary via the harness.

## Validation against real decomps

Beyond the synthetic corpus, the harness can be pointed at real translation units from
`reference_projects/` (other GC decomps) and FFCC itself: compile a TU with `mwcc-rs` and diff
against the project's known-good target object. The `pppRand*` family is the canonical hard test.

## Layout

```
src/lexer.rs    src/parser.rs    src/codegen.rs   # front to back end
src/ppc.rs      # PPC/Gekko instruction encoders (verified vs oracle)
src/elf.rs      # ELF32 big-endian PPC object writer
corpus/         # A/B test programs (the milestone ladder)
harness/abtest.sh
oracle_probe/   # scratch: real-compiler output samples used to derive encodings
```
