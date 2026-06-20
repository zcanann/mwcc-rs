// A string-pointer global pools each distinct string literal as an anonymous
// local `@N` object in .sdata (bytes + NUL, emitted just before the pointer that
// first uses it), the pointer relocating to it. Identical strings share one `@N`
// (mwcc -str reuse). A function-free unit (the @N numbering is bumped by a
// function's anonymous symbols — deferred).
char* strpool_p = "hi";
char* strpool_table[] = { "alpha", "beta", "alpha" };
