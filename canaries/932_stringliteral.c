// A small string literal becomes an anonymous `@N` object in `.sdata` that the referencing
// instruction reaches with an `R_PPC_EMB_SDA21` `li` (SDA21 addressing). The unit's string
// resolver numbers each NEW string at the FRONT of its function's `@N` block (before that
// function's constants and unwind entries) and pools identical strings across the whole unit
// (`-str reuse`): a reused string consumes no new `@N`.
//
// One function here introduces every distinct string (`@5` "hi", `@6` "A", `@7` "abcdefg" — the
// last is 7 chars + NUL = 8 bytes, right at the small-data size boundary, so still SDA21). The
// remaining functions only REUSE those pooled strings, so no second `@N` slot is cut and a lone
// returned string is byte-exact too.
//
// DEFERS (no wrong bytes, roadmap): a string LARGER than the small-data threshold (>8 bytes —
// mwcc switches to ADDR16 `lis`/`addi`); a SECOND function that introduces its own NEW string
// (per-function string-symbol interleaving with unwind entries isn't ordered yet); a string
// alongside a pooled constant in the same function (same symbol-order seam).
void  take(char *);
void  introduce(void) { take("hi"); take("A"); take("abcdefg"); }  // @5, @6, @7 — all new, one function
void  reuse_hi(void)  { take("hi"); }                              // pooled -> reuses @5, no new slot
char *ret_hi(void)    { return "hi"; }                             // pooled -> reuses @5, li r3,@5
void  reuse_ab(void)  { take("A"); take("abcdefg"); }              // pooled -> reuses @6, @7
