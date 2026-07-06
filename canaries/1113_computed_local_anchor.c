// Boundary guard for the "computed local anchored by liveness" defer (driver.rs). A
// plain-int local with a COMPUTED (Binary) initializer, used as the LEFT operand of a
// commutative add/bitwise op whose right operand is a constant or a variable from the
// initializer (`int b=a*a; return b+a;` / `b+3` / `b|a`), needs the register allocator
// (#20) — mwcc anchors the live-reused register — so it DEFERS. These NEIGHBORS place
// operands exactly as mwcc does and stay byte-exact; this canary locks them so the defer
// is not re-broadened to swallow them (an earlier attempt wrongly caught floats + chars).
int neighbor_independent(int a, int c) { int b = a * a; return b + c; }   // independent param -> add r3,r0,r4
int neighbor_multiply(int a)           { int b = a * a; return b * a; }   // `*` (not the anchor set)
int neighbor_subtract(int a)           { int b = a * a; return b - a; }   // `-` (non-commutative)
int neighbor_self(int a)               { int b = a * a; return b + b; }   // right is the local, not an init var
int neighbor_leaf_left(int a)          { int b = a * a; return a + b; }   // leaf on the left already anchors it
// A FLOAT computed local (`fadds` operand order matches) and a NARROW/LOAD-init local
// (extended value is register-resident) are byte-exact — covered by 384_vtfmuladd,
// 834_narrowlocalreturn, 837_narrowinglocal; the int-with-computed-init guard excludes them.
