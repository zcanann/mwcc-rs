// Logical (&&/||) condition in a guarded return short-circuits straight into the two return
// blocks — mwcc branches each term to the taken/fall-through return rather than computing the
// operator as a 0/1 value. Params: a=r3, b=r4, c=r5, d=r6, x=r5/r7, y=r6/r8.
//
//   &&: cmpwi rA,0; beq FALL; cmpwi rB,0; beq FALL; <X>; blr; FALL: <Y>; blr
//   ||: cmpwi rA,0; bne TAKEN; cmpwi rB,0; beq FALL; TAKEN: <X>; blr; FALL: <Y>; blr
//
// When the taken value is already in the result register, the deciding AND term becomes a
// conditional return and the separate taken block is dropped (`cmpwi rB,0; bnelr; <Y>; blr`).
// Restricted to a single &&/|| chain of leaf/comparison terms with leaf-or-constant returns;
// mixed `a&&b||c`, computed arms, and `||` with the taken value in the result register defer.
int and_const(int a, int b)               { if (a && b) return 1; return 0; }
int or_const(int a, int b)                { if (a || b) return 1; return 0; }
int and_leaf(int a, int b, int x, int y)  { if (a && b) return x; return y; }
int and_taken_in_r3(int a, int b)         { if (a && b) return a; return b; }   // bnelr form
int and_three(int a, int b, int c)        { if (a && b && c) return 1; return 0; }
int or_three(int a, int b, int c)         { if (a || b || c) return 1; return 0; }
int and_compare(int a, int b, int c, int d) { if (a > b && c > d) return 1; return 0; }
int or_compare(int a, int b, int c, int d)  { if (a < b || c == d) return 1; return 0; }

// Edge cases (all byte-exact): float-comparison terms, negated terms, and a 4-term chain.
int and_float_cmp(float a, float b, float c, float d) { if (a > b && c > d) return 1; return 0; }
int and_negated(int a, int b)                         { if (!a && b) return 1; return 0; }
int and_four(int a, int b, int c, int d)              { if (a && b && c && d) return 1; return 0; }
int and_pointer(int *p, int b)                        { if (p && b) return 1; return 0; }
