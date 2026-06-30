// A float/double deref (or struct member) combined with a CONSTANT of the SAME precision: the value
// and the constant load into the dest/scratch float registers (a memory operand has no home reg like
// a variable). A commutative op (+, *) leads with the constant (into the dest) then the value
// (scratch): `lfs/lfd f1,const; lfs/lfd f0,(p); op f1,f1,f0`. A non-commutative op (-, /) leads with
// the VALUE (dest) then the constant (scratch): `op f1,f1,f0` = value OP const. place_float_located_
// operands now threads the `double` flag (so a double pointee loads an 8-byte `lfd` constant, not a
// 4-byte `lfs`) and orders subtract/divide value-first. Const-on-left already led with the constant.
// (All DIFF at HEAD before this — the branch only special-cased a constant against a *variable* leaf.)
// SEPARATE pre-existing gaps, not fixed here: a MIXED-precision literal (`double f(float*p){*p+1.0;}`)
// — FloatLiteral carries no float/double tag, so the double constant reads as single; and a
// VARIABLE-INDEX subscript `a[i] OP const` — is_float_located does not cover Index, so it defers.
float  f_add(float *p)         { return *p + 1.0f; }
float  f_sub(float *p)         { return *p - 1.0f; }   // lfs f1,(p); lfs f0,1; fsubs f1,f1,f0
float  f_div(float *p)         { return *p / 3.0f; }
double d_add(double *p)        { return *p + 1.0; }     // lfd constant (8 bytes)
double d_sub(double *p)        { return *p - 1.0; }
double d_mul(double *p)        { return *p * 2.0; }
double d_div(double *p)        { return *p / 3.0; }
float  f_rsub(float *p)        { return 2.0f - *p; }    // constant on the left
double d_rdiv(double *p)       { return 6.0 / *p; }
struct S { float x; };
float  member_sub(struct S *s) { return s->x - 1.0f; }
