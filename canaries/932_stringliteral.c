// A string literal becomes an anonymous `@N` object that the referencing instruction addresses by
// its size. The unit's string resolver numbers each NEW string at the FRONT of its function's `@N`
// block (before that function's constants and unwind entries) and pools identical strings across the
// whole unit (`-str reuse`): a reused string consumes no new `@N`.
//
//   - a string within the small-data threshold (<= 8 bytes incl. NUL) lands in `.sdata`, reached by
//     a single SDA21 `li` (`abcdefg` is 7 chars + NUL = 8 bytes, right at the boundary — still SDA21);
//   - a larger string lands in `.data`, reached by ADDR16 `lis`/`addi` (`@ha`/`@l`), like a large
//     global array's base.
//
// One function here introduces every distinct string; the rest only REUSE those pooled strings, so
// no second `@N` slot is cut and a lone returned string is byte-exact too. This exercises small +
// large in one function, cross-function reuse of both, and a returned large string.
//
// DEFERS (no wrong bytes, roadmap): a SECOND function that introduces its own NEW string
// (per-function string-symbol interleaving with unwind entries isn't ordered yet); a string
// alongside a pooled constant in the same function (same symbol-order seam).
void  take(char *);
void  introduce(void)  { take("hi"); take("abcdefg"); take("longer string here"); }  // @5 SDA21, @6 SDA21 (8B), @7 ADDR16
void  reuse_small(void) { take("hi"); }                             // pooled -> reuses @5
void  reuse_large(void) { take("longer string here"); }            // pooled -> reuses @7 (ADDR16)
char *ret_large(void)   { return "longer string here"; }           // pooled -> reuses @7, lis/addi
