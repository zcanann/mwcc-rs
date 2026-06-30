// A NARROW (8/16-bit) UNSIGNED load (deref/element/member) shifted right: `(unsigned char/short)*p
// >> n`. The narrow value promotes to a SIGNED int before the shift, so mwcc emits the ARITHMETIC
// `srawi` (`lbz/lhz r0; srawi r3,r0,n`) — not the logical `srwi` ours picked from the operand's own
// unsigned type. The loaded value is non-negative so the result is identical, but the instruction
// differs (a byte diff, pre-existing — found by a broad post-regression DIFFERS hunt). emit_shift_right
// (arithmetic.rs) now ORs in is_narrow_unsigned_load (a new helper). Unchanged: a signed deref is
// already srawi; a WIDE unsigned (`unsigned int`) stays srwi (no promotion to signed); a narrow
// unsigned LEAF still uses the fused rlwinm. After this, a broad signed/unsigned narrow-deref scan
// across ~/-/!/>>/+/==/< is fully byte-exact-or-defer (0 DIFF).
int uc_shr(unsigned char *p)         { return *p >> 1; }
int us_shr(unsigned short *p)        { return *p >> 2; }
int uc_elem_shr(unsigned char *a)    { return a[1] >> 1; }
struct S { unsigned char x; int y; };
int uc_member_shr(struct S *s)       { return s->x >> 2; }
