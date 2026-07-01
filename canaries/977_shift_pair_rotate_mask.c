// An UNSIGNED shift pair `(x << k) >> n` (logical) collapses to a single rotate-and-mask
// `rlwinm x, (32+k-n)%32, n, (k>=n ? 31-(k-n) : 31)`, not two shifts. `k==n` is the
// zero-extend idiom `clrlwi x,n` (`(x<<24)>>24` -> `x & 0xFF`). Previously emitted two
// shifts (`slwi; srwi`), a byte-DIFF. The SIGNED form sign-extends and stays a shift pair.
unsigned zext_byte(unsigned x)  { return x << 24 >> 24; }   // clrlwi 24  (x & 0xFF)
unsigned zext_half(unsigned x)  { return x << 16 >> 16; }   // clrlwi 16  (x & 0xFFFF)
unsigned zext_nib(unsigned x)   { return x << 28 >> 28; }   // clrlwi 28  (x & 0xF)
unsigned kn_gt(unsigned x)      { return x << 24 >> 16; }   // rlwinm x,8,16,23
unsigned kn_lt(unsigned x)      { return x << 16 >> 24; }   // rlwinm x,24,24,31
