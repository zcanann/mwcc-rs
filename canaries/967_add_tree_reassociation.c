// mwcc REASSOCIATES an all-`+` chain of register leaves `v1+v2+...+vN` (N>=4) to
// `v1 + left-fold(v2..vN)`, allocated as:
//   add r0,R2,R3; mr R2,R1; <fold R4..R(N-1) into r0>; add D,r0,RN; add D,R2,D
// (v1 kept in v2's freed register across the folds that would clobber the destination). The
// codegen reproduces this directly for distinct register-resident operands. N<=3 is the normal
// left-form; a `*` operand, a repeated operand, or a constant/global leaf falls out (deferred or
// its own path). A mixed +/- chain is a plain left-fold, not reassociated.
int sum4(int a, int b, int c, int d)               { return a + b + c + d; }
int sum5(int a, int b, int c, int d, int e)        { return a + b + c + d + e; }
int sum6(int a, int b, int c, int d, int e, int f) { return a + b + c + d + e + f; }
int sum4_local(int a, int b, int c, int d)         { int s = a + b + c + d; return s; }
