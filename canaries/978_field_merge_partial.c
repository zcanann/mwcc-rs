// `(a & maskA) | (b & maskB)` with disjoint contiguous fields merges via `rlwimi`.
// When the two masks tile the whole word (full coverage) mwcc moves the base raw and
// rlwimi overwrites the other field. With PARTIAL coverage (bits outside both fields
// must be zero) the base is masked to its field first: `(a&0xff00)|(b&0xff)` ->
// `mr r0,a; clrlwi b; rlwimi r0`. Previously only full coverage was handled (partial
// fell back to two masks + `or`, a byte-DIFF).
unsigned byte_fields(unsigned a, unsigned b)  { return (a & 0xFF00) | (b & 0xFF); }
unsigned nibbles(unsigned a, unsigned b)      { return (a & 0xF0) | (b & 0x0F); }
unsigned top_bottom(unsigned a, unsigned b)   { return (a & 0xFF000000) | (b & 0xFF); }
unsigned full_halves(unsigned a, unsigned b)  { return (a & 0xFFFF0000) | (b & 0xFFFF); }
