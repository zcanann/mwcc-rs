// A commutative float op (+/*) with a NEGATE operand diverges from mwcc and DEFERS: `-a+b` keeps the
// fneg but mwcc puts the fneg RESULT first in the fadds (we swap), and `-(a*b)+c` contracts to a
// single fnmsubs that we emit un-fused. Unlike integers, float `-a+b` != `b-a` in IEEE (signed zero),
// so it is NOT folded to a subtract. A SUBTRACT keeps its byte-exact form, as do fused multiply-adds:
float sub_neg(float a, float b)        { return -a - b; }      // fneg + fsubs
float fnms(float a, float b, float c)  { return c - (a * b); } // fnmsubs (c - a*b)
float fms(float a, float b, float c)   { return a * b - c; }   // fmsubs
float fmadd(float a, float b, float c) { return a * b + c; }   // fmadds
float neg(float a)                     { return -a; }          // fneg
