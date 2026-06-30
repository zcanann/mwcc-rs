// Three independent fixes, each for a shape that was DIFF at HEAD (found by a fresh broad DIFF hunt):
// (1) `a * -C` for a power-of-two C is `-(a << log2 C)` — `slwi r0,a,n; neg d,r0`, not `mulli d,a,-C`.
// (2) `f op f` over an IDENTICAL float MEMORY load (`*p+*p`, `*p**p`, `a[i]+a[i]`) loads ONCE into
//     the scratch then applies the op to it twice (`lfs/lfd f0,(p); fadds/fmuls d,f0,f0`), like the
//     integer identical-load idiom — not a double load.
// (3) is_double_value now sees through a `double*` deref/subscript, so double-pointer arithmetic
//     uses the double `fadd/fmul` (not the single `fadds/fmuls`) — fixing double *p OP *q etc.
// NOTE: a float/double deref combined with a CONSTANT (`*p - 1.0f`, `double *p * 2.0`) is a SEPARATE
// pre-existing DIFF in place_float_operands (it only special-cases a constant against a *variable*
// leaf, not a memory load) — recorded for a dedicated fire, not addressed here.
int    mul_neg2(int a)                    { return a * -2; }   // slwi r0,r3,1; neg r3,r0
int    mul_neg16(int a)                   { return a * -16; }  // slwi r0,r3,4; neg r3,r0
float  f_self_add(float *p)               { return *p + *p; }  // lfs f0; fadds f1,f0,f0
float  f_self_mul(float *p)               { return *p * *p; }
double d_self_add(double *p)              { return *p + *p; }  // lfd f0; fadd f1,f0,f0
double d_self_mul(double *p)              { return *p * *p; }
double d_two_loads(double *p, double *q)  { return *p + *q; }  // lfd; lfd; fadd
double d_index_add(double *a, int i)      { return a[i] + a[i]; }
