# mwcc-rs

A byte-exact reimplementation, in Rust, of **Metrowerks CodeWarrior for Embedded PowerPC** (`mwcceppc`) — the compiler that built Nintendo GameCube and Wii games — for use in decompilation.

The goal is narrow and absolute: for every configured translation unit in
`reference_projects/*`, `mwcc-rs` must emit the same relocatable object as the
real compiler, byte for byte. Equivalent code or an exact `.text` section is
useful diagnostic evidence, but it is not parity. The build registry currently
contains eight ordinarily selectable GameCube profiles and seven explicitly
experimental GameCube/Wii identities, all driven by one parameterized compiler.

GC/1.3.2r remains recognized for compatibility, but it is not a required parity gate: it was a hacked Animal Crossing compiler variant that disabled `.rodata` pooling, and the underlying stock GC/1.3.2 bug is now understood without relying on that build.

## Why this exists

A decompilation is verified by recompiling reconstructed source and checking that it reproduces the original game's machine code exactly. That makes the *compiler* a hard dependency: you must own the precise build the game shipped with, and you must be able to coax its precise register allocation and instruction scheduling. Two problems follow:

1. **Some builds are lost.** The real `mwcceppc` is a closed 2002 Windows binary. Point builds exist that were never preserved (an FFCC-era 1.3.1, for instance); no amount of searching produces them.
2. **The real compiler is opaque and unmodifiable.** When its allocator or scheduler makes a choice the reconstructed source can't reproduce, there is no recourse — you cannot inspect why, and you cannot change it.

An open, inspectable, *modifiable* compiler that matches `mwcceppc` removes both walls. It lets projects match functions the stock binary cannot, it can be diffed and adjusted to reconstruct missing builds, and it serves the whole GameCube/Wii decomp community — many projects hit the same compiler wall.

## How it works: the differential oracle

The real `mwcceppc` is the **source of truth**. Development is a tight test-driven loop against it:

1. Add a small program to `canaries/`.
2. The oracle compiles it with **both** the real compiler and `mwcc-rs`, and compares the `.text` disassembly.
3. Make `mwcc-rs` match, exactly. Grow the canary set.

```sh
cargo build --release
./target/release/mwcc-oracle 1.3.2     # PASS/FAIL per canary, with the diff on failure
./target/release/mwcc-oracle 1.3       # any supported build: 1.3 1.3.2 1.3.2r 2.0 2.5 2.6 2.7
tools/vdiff.sh canaries/112_rdchar.c 1.3 1.3.2   # diff what two real builds emit
```

The oracle needs a decomp checkout for the real toolchain — `wibo`, the compiler set, and `powerpc-eabi-objdump`. Point it at one with `FFCC=/path/to/decomp`. Nothing about the *design* is decomp-specific; that's just where the reference binaries live.

There is a standing rule: **fail honestly**. When a construct is not yet supported, the relevant phase returns a diagnostic. It never emits plausible-but-wrong bytes — a silently-wrong compiler is worse than one that stops.

### Measuring reference-project parity

Canary totals and a hand-picked green gate are regression aids, not a measure of
overall parity. The persistent frontier loop uses every configured translation
unit in `reference_projects/*` as its denominator, spends each iteration on
known `DIFF`, `DEFER`, and harness failures before drawing from untested work,
and reserves a small rotating sample of previous byte matches for regression
detection:

```sh
cargo build -p mwcc
# Default edit loop: rotate a failure-biased sample; prior BYTE rows need not stay green.
python3 tools/parity_loop.py --epoch 0

# Periodic scorecard: rerun the frozen representative sample from scratch.
python3 tools/parity_loop.py --audit-only --audit-size 384 --rerun

# Deliberately run both when cutting a milestone/release scorecard.
python3 tools/parity_loop.py --with-audit --audit-size 384 --rerun
```

The scorecard leads with the literal completion proof: authoritative exact
translation-unit configurations over the full configured corpus, and project
matrices for which every configuration is proven exact. The fixed audit is a
simple random sample without replacement, so it reports a corpus estimate and
confidence interval with an explicit denominator. Measurement-unknown rows
(missing dependencies, invalid captured configurations, harness failures, or
non-authoritative synthetic comparisons) widen separate identification bounds;
they are never silently counted as compiler failures or successes. Deterministic
out-of-sample canaries cover every project x compiler-version x language cell,
while remaining outside the statistical estimator.

The edit loop's compact output always labels these evidence layers separately.
In particular, its failure-biased queue is explicitly *not* a parity estimate;
if the fixed audit has not been run for the current compiler+harness fingerprint,
the report says so instead of substituting a canary or work-queue pass count.

The report also separates `BYTE`, `DIFF`, `DEFER`, `HARNESS`, unsupported builds,
and untested configurations, with language, compiler-version, and project
breakdowns. Full-object parity remains the credited success measure. A separate
code-plus-text-relocation diagnostic includes explicitly labeled, non-credit
`-sym off` projections for configurations blocked only by unimplemented debug
sections; this preserves backend signal without misrepresenting those objects as
1:1 matches. The report also exposes empty-code rows rather than allowing them to
inflate that diagnostic. It reports source coverage so files absent from project
build metadata cannot disappear from the denominator. Results are keyed by stable
compiler inputs and retained under `target/reference-parity/frontier/`; after a
compiler change, unresolved work remains in the frontier while observations for
the new binary are kept separate. Each run executes an immutable copy of the
fingerprinted compiler, so a concurrent rebuild cannot mix binaries inside one
cache. Increment `--epoch` to rotate equally ranked work. `tools/gate.sh` remains
available as an occasional exhaustive regression check, but it is intentionally
not on this inner loop.

## Architecture

A Cargo workspace split three ways. The discipline is that **data and transforms are different crates**: a *representation* is the data a phase produces (a noun); a *pipeline* crate is a transform named for what it converts (`source-to-tokens`). You can read the whole compiler off the crate list.

```
crates/
  foundation/          shared vocabulary, no pipeline logic
    mwcc-core            diagnostics, source spans, the Compilation result type
    mwcc-target          PowerPC/Gekko register file + the EABI calling convention
    mwcc-versions        the build registry + a per-build CodegenProfile trait
    mwcc-object          ELF32 big-endian PowerPC object writer

  representations/     the data each phase produces
    mwcc-tokens          lexical tokens
    mwcc-syntax-trees    the parsed program
    mwcc-machine-code    PowerPC instructions (structured) + their encodings

  pipeline/            the transforms between representations
    mwcc-source-to-tokens               lexing
    mwcc-tokens-to-syntax-trees         parsing
    mwcc-syntax-trees-to-machine-code   lowering, selection, register assignment
    mwcc-machine-code-to-object         encoding + object emission

apps/    mwcc            the compiler driver (mwcceppc-compatible CLI)
harness/ mwcc-oracle     the differential oracle described above
canaries/                one C program per capability under test
```

Within a crate, every `lib.rs` is a visibility exporter — module wiring plus the public entry point — and the work lives in focused, clearly-named files. The code generator, for instance, is one `Generator` whose methods are grouped by theme across `body.rs`, `expressions.rs`, `arithmetic.rs`, `comparisons.rs`, `control_flow.rs`, `narrow.rs`, `casts.rs`, `placement.rs`, `floats.rs`, … each a further `impl Generator`. Per-build differences are not scattered `if version ==` checks: each build carries a `CodegenProfile` whose default methods are the 2.4.x mainline, so a divergent build (GC/1.3's unsigned `char`, GC/2.0p1's int→float scheduling) overrides only the method that changed. Branching support to a new build is "add a profile, override one method" — the shared path and every other build stay untouched.

Why structured machine code instead of raw words: the register **allocator** and instruction **scheduler** are where byte-matching is actually won or lost, and they must *inspect and rewrite* the instruction stream before it is encoded. They get their own pipeline crates as the language grows; the `mwcc-machine-code` representation is the seam they plug into.

On dependencies: the codegen path is deliberately bespoke. General code generators (Cranelift, LLVM) optimize for *good* code and ship their own allocator and scheduler — the exact passes we must reproduce — so they cannot help here and would actively prevent matching. We also own the object bytes rather than reaching for a general object crate, because decomp tooling keys on exact section ordering, symbol order, alignment, and the Metrowerks `.comment` record. (A general object writer becomes worth adopting once relocations and `.data`/`.sdata` arrive; see M3.)

## Inspecting a compilation

Every phase can dump an artifact, which is how you debug a byte mismatch — you can see precisely where our decision diverged from the oracle's:

```sh
mwcc -c canaries/02_add.c -o add.o --emit-artifacts ./build
#  00_build.txt  01_tokens.txt  02_syntax_tree.txt  03_machine_code.txt  04_object.txt
```

## Status

Canaries are regression probes, not a parity score. Current status comes from
the denominator-qualified reference-project scorecard described above; a
failure-biased frontier count must never be reported as overall progress. The
compiler reproduces MWCC's object format and instruction stream across a broad
and growing C/C++ subset, including:

- **Multiple builds from one generator.** A cross-build survey (`tools/vdiff.sh`,
  ~320 probed forms) established that mwcceppc builds 53…108 share a single code
  generator distinguished by two observable knobs. The main one is the default
  signedness of plain `char` (unsigned in GC/1.3 build 53, signed from build 81
  on): it lives on `CompilerBuild`, is threaded through one `signed_of(Type)`
  query, and cascades correctly into read/operand extension, `>>`/`/`/`%`
  strength reduction (including the narrow-unsigned `rlwinm` fusion), comparison
  folding, and the int→float bias with no scattered version checks. The second is
  a single int→float instruction-scheduling order that distinguishes GC/2.0p1.
  The oracle pins our codegen to the build under test.

- **EABI & expressions** — integer/float args and returns; `+ - * / %` (signed and
  unsigned), bitwise `& | ^ ~`, shifts `<< >>` (sign-aware), comparisons, unary `- ~ !`.
- **The register allocator** — a free-register pool with a live/reserved set: a
  binary node computes its left side into the lowest free register while the right
  side's inputs stay reserved, the right into the scratch (`r0`/`f0`). Handles
  shared inputs, dead-input reuse, and the consumer-dependent operand placement
  (`addi` keeps operands in the destination, `rlwinm`/logical route through `r0`).
  *Matching mwcc's exact register coloring is the core research target.*
- **Instruction selection** — `slwi`/`srwi`/`srawi` for shift-by-constant, `rlwinm`
  for contiguous masks, `mulli`/`addi` immediate folds, `andc`/`orc`, fused
  float multiply-add (`fmadds`/`fmsubs`/`fnmsubs`), and identity/strength folds
  (`a*-1`→`neg`, `a+0`→`a`, negated-literal constants).
- **Control flow** — ternary `?:`, `if`-return guards (single → select, chained →
  return blocks), comparison conditions (`cmpw`/`cmplw` + the negated branch),
  conditional returns (`bnelr`/`bgtlr`), forward branches with encode-time offset
  resolution, float selects (`fcmpo`).
- **Casts & types** — int↔float (the FFCC `randchar` magic-constant conversion, at
  the `.text` level), stack frames (`stwu`/`addi`), narrow `char`/`short`/`unsigned`
  with sign/zero-extension, and narrow arithmetic — operands widened before use
  (`extsb`/`extsh`/`clrlwi`) with the anchor-in-place / other-through-`r0` placement
  mwcc uses for two narrow operands.

What's deliberately *not* matched yet — and where the hard, large subsystems lie:
mwcc's **optimizer** (CSE, algebraic factoring `a*b+a*c`→`a*(b+c)`, chain
re-association, value-range), the **instruction scheduler** (it reorders within a
block), **loop unrolling** (a simple `while` becomes an 8× `mtctr`/`bdnz` loop at
-O4), full **object metadata** (`.sdata2` constants, `R_PPC_EMB_SDA21` relocations,
the `extab`/`extabindex`/`.mwcats` sections), and the **C++** frontend.

## Roadmap

Each milestone is a canary tier that must stay 100% byte-exact before the next begins.

- **M1 — locals, stack frames, the register allocator.** Prologue/epilogue (`stwu` / `mflr` / `stw…` / `lwz` / `mtlr` / `blr`), a spill model, and the allocator. This is the core research target: matching mwcc's exact register coloring is the single hardest part of the whole project.
- **M2 — control flow and the instruction scheduler.** `if` / `while` / `for`, comparisons and conditional branches, and the scheduler that decides instruction order.
- **M3 — memory, types, and the constant pool.** Pointers, structs, arrays, the narrow integer types, loads and stores, `.data` / `.sdata` and **relocations**, and the float/double constant pool.
- **M4 — calls and the full ABI.** Function calls, argument marshalling, varargs, aggregate returns.
- **M5 — a C++ subset.** Metrowerks name mangling, member functions, references, `inline`, simple templates — enough to compile real decomp translation units.
- **M6 — multiple builds.** *In progress:* codegen is parameterized by `CompilerBuild` and eight GameCube builds (GC/1.3 … 2.7, including 2.0p1) are byte-exact today. Remaining: the GC/3.0 alpha line (mwcceppc 4.1/4.2, ~11 canary diffs), the 2.3.x line (GC/1.0–1.2.5, a distinct generator), and a reconstructed 1.3.1.

## Canaries

A canary is the smallest program that pins one compiler behavior. Because the real compiler defines the expected output, canaries are just source — the oracle supplies the answer. Name a canary for the behavior it exercises, not the program that exposed it. Beyond the synthetic set, the oracle can be pointed at real translation units and diffed against a project's known-good objects; that is the ultimate test.

## Conventions

- **Real words.** `expression`, not `expr`; `arguments`, not `args`; `character`, not `ch`. Names should read without compiler-insider shorthand.
- **Honest phases.** Lex, parse, lower, select, allocate, schedule, emit — each does one nameable thing, and says so when it can't.
- **Own the bytes.** The output is the product; the encoder and the object container are ours so every byte is accountable.
- **The oracle is the authority.** No guessing about what mwcc does — every claim is a diff against the real compiler.

## License

Dual-licensed under MIT or Apache-2.0.
