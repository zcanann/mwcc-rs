// mwcc lays the small zero-BSS `.sbss` out — and emits its symbols — as: EXPLICITLY
// zero-initialized globals (`int a = 0;`) in DECLARATION order, interleaved with the
// initialized `.sdata` globals up front, THEN the UNINITIALIZED globals (`int b;`) in
// REVERSE declaration order after the functions. (An all-uninitialized `.sbss` therefore
// just reverses.) An explicit `= 0` is a zero VALUE — it still lands in `.sbss`, not
// `.sdata`, but orders like an initializer, not like an uninitialized global.
//
// Symbol order below (mwcc): initialized `sd` and explicit-zero `ez*` in source order,
// then `f`, then the uninitialized `un*` reversed — verified across GC 2.6/2.0/1.3.2.
int          sd = 7;    // .sdata (initialized)   — front, decl order
int          ez1 = 0;   // .sbss  (explicit zero) — front, decl order
int          un1;       // .sbss  (uninitialized) — trails, reversed
int          ez2 = 0;   // .sbss  (explicit zero) — front, decl order
int         *ezp = 0;   // .sbss  (explicit-zero pointer)
int          un2;       // .sbss  (uninitialized) — trails, reversed
static int   sez = 0;   // static .sbss explicit zero — local symbol, forward
static int   sun;       // static .sbss uninitialized — local symbol, reversed

// A trivial function so the object has a `.text` symbol between the front (initialized +
// explicit-zero) run and the trailing reversed uninitialized run. The globals are emitted
// whether or not they are referenced, so a single read suffices.
int use(void) { return sd; }
