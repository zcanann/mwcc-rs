// A zero-initialized ARRAY is NOT coalesced into `.sbss`/`.bss` the way a zero SCALAR is:
// mwcc keeps its materialized zero bytes in initialized data — `.sdata` when small (<= 8),
// `.data` when large (> 8) — laid out in the initialized front group in declaration order.
// A zero-initialized SCALAR (`int s0 = 0;`) still coalesces to `.sbss`. An uninitialized
// array/scalar stays in `.sbss`/`.bss`. (The discriminator is array-vs-scalar, not size:
// `int a[2] = {0,0};` is 8 bytes yet lands in `.sdata`, while `double d = 0;` is also 8
// bytes and lands in `.sbss`.)
int    za_small[2] = {0, 0};      // zero array,  8 bytes  -> .sdata (materialized zeros)
int    za_large[3] = {0, 0, 0};   // zero array, 12 bytes  -> .data  (materialized zeros)
int    nz[2]       = {1, 2};      // nonzero array         -> .sdata
int    s0          = 0;           // zero scalar           -> .sbss  (coalesced, explicit zero)
double d0          = 0;           // zero scalar (8 bytes) -> .sbss  (coalesced)
int    un_scalar;                 // uninitialized scalar  -> .sbss  (reversed run)
int    un_array[2];               // uninitialized array   -> .sbss  (reversed run)

// A trivial function so the object carries a `.text` symbol between the initialized front
// run and the trailing reversed uninitialized run; globals are emitted regardless of use.
int touch(void) { return s0; }
