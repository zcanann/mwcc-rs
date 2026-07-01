// The offset-0 element of a SMALL (<= 8 byte) float global array reads with a single folded
// SDA21 float load — `lfs f1, fa@sda21(r0)` — the float counterpart of the int/short index-0
// fold (954). Previously it was misclassified as an "integer memory load in a float context"
// (is_float_value didn't recognize a file-scope float-array element, whose base is not in the
// pointer map) and deferred.
//
// DEFERS (no wrong bytes): a NON-zero-offset element (`fa[1]`) or a LARGE (> 8 byte) float/double
// array element needs a separate GPR base distinct from the FPR destination — a follow-up.
float fa[2];   // 8-byte float array -> SDA21
float fb[1];   // 4-byte float array -> SDA21

float first(void)  { return fa[0]; }   // lfs f1, fa@sda21(r0)  (folded)
float only(void)   { return fb[0]; }   // lfs f1, fb@sda21(r0)  (folded)
