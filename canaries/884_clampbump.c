// A clamp-to-zero SELECT `(x REL 0) ? x : 0` is a ternary and advances mwcc's anonymous-@N counter
// by 3, like the other ternary forms (881 bool, 882 comparison, 883 sign-mask). The clamp
// instructions already matched mwcc; only a frame fn's extab/extabindex @N was short by 3.
// try_emit_sign_clamp now bumps anonymous_label_bump by 3. The byte-exact relations are `> 0`
// (max: neg;andc;srawi;and), `<= 0` (min: neg;orc;srawi;and), and the `> 0 ? 0 : x` mirror.
// (`< 0`/`>= 0` clamps match @N now too but carry separate keystone diffs — epilogue-restore
// ordering and free-register selection respectively — so they are not exercised here.)
extern void g(void);
int clampmax(int a) { g(); return a > 0 ? a : 0; }
int clampmin(int a) { g(); return a <= 0 ? a : 0; }
int clampmaxm(int a){ g(); return a > 0 ? 0 : a; }
