// String literals become anonymous `@N` objects, numbered at the FRONT of each function's `@N`
// block (before that function's constants and unwind entries) and pooled across the whole unit
// (`-str reuse`): a reused string consumes no new `@N`. A string within the small-data threshold
// (<= 8 bytes incl. NUL) lands in `.sdata`, reached by a single SDA21 `li`; a larger one lands in
// `.data`, reached by ADDR16 `lis`/`addi` (`@ha`/`@l`).
//
// The string SYMBOLS interleave PER-FUNCTION with each function's constant/unwind symbols the way
// mwcc lays them out (the writer emits them in its `@N` run, not grouped in the data section). So:
//   - several functions may each introduce their OWN new string (symbols interleave correctly);
//   - a string may share a function with a pooled `.sdata2` constant;
//   - small and large strings mix, and later functions reuse earlier pooled strings.
//
// DEFERS (no wrong bytes, roadmap — unrelated to strings): a value kept live across the call needs
// the callee-saved register allocator, so `alpha`/`beta`/`mixed` keep their bodies leaf-simple.
void  take(char *);
void  alpha(void) { take("aa"); take("wide alpha string"); }  // @5 SDA21 (.sdata), @6 ADDR16 (.data)
void  beta(void)  { take("bb"); }                             // a SECOND function introducing a new string
void  reuse(void) { take("aa"); take("bb"); }                 // reuses both pooled strings — no new @N
float mixed(void) { take("cc"); return 1.5f; }                // a string alongside a pooled .sdata2 constant
