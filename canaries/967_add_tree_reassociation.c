// mwcc REASSOCIATES an all-`+` chain of register leaves `v1+v2+...+vN` (N>=4) to
// `v1 + left-fold(v2..vN)`, allocated as:
//   add r0,R2,R3; [mr R2,R1 iff v1's reg == destination]; <fold R4..R(N-1) into r0>;
//   add D,r0,RN; add D,v1reg,D
// v1 only moves aside when it already sits in the destination register (else it survives the
// folds in place). This covers ANY operand order, not just v1==first-param. N<=3 is the normal
// left-form; a `*`/repeated/constant/global/frame leaf falls out (deferred or its own path).
int sum4(int a, int b, int c, int d)               { return a + b + c + d; }   // v1 == destination
int reorder1(int a, int b, int c, int d)           { return b + a + c + d; }   // v1 != destination
int reorder2(int a, int b, int c, int d)           { return c + d + a + b; }
int reorder3(int a, int b, int c, int d)           { return d + c + b + a; }
int acbd(int a, int b, int c, int d)               { return a + c + b + d; }   // mr target = min(R2,R3)
int sum5(int a, int b, int c, int d, int e)        { return a + b + c + d + e; }
int shuffle5(int a, int b, int c, int d, int e)    { return b + c + a + d + e; }
int sum6(int a, int b, int c, int d, int e, int f) { return a + b + c + d + e + f; }
