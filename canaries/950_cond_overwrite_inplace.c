// The conditional-OVERWRITE-then-return idiom `T y = v; if (c) y = NEW; return y;` (no else),
// where the initializer `v` is a parameter already resident in the result register (r3). mwcc
// keeps `v` in r3 with NO move, tests `c`, and issues a conditional RETURN on the INVERSE
// (`b<!c>lr`) that returns `v` in place; the taken path falls through to `<NEW into r3>; blr`.
// This is the min / max / abs / clamp shape. NEW is emitted by the general tail evaluator, so
// every operator lands byte-exactly: `neg`, `mr` (a variable), `li` (a constant), `add`/`mullw`
// (a computed value).
//
// DEFERS (no wrong bytes): a non-int return, an initializer NOT already in the result register,
// an `else` arm, or a multi-statement then-body — each is a different mwcc layout.
int absval(int x)          { int y = x;  if (x < 0)  y = -x;    return y; }  // cmpwi;bgelr;neg;blr
int maxval(int a, int b)   { int m = a;  if (b > a)  m = b;     return m; }  // cmpw;blelr;mr;blr
int floor0(int x)          { int y = x;  if (x < 0)  y = 0;     return y; }  // cmpwi;bgelr;li 0;blr
int ceil10(int x)          { int y = x;  if (x > 10) y = 10;    return y; }  // cmpwi;blelr;li 10;blr
int addcond(int a, int b)  { int y = a;  if (a < b)  y = a + b; return y; }  // cmpw;bgelr;add;blr
