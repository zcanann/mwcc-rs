// A switch on an EXPRESSION scrutinee (`switch(n & 3)`, the math-file quadrant pattern) now compiles.
// mwcc evaluates the scrutinee into the general scratch r0 (`clrlwi r0,r3,30` for n&3), then runs the
// same binary-search comparison tree it uses for a plain-variable scrutinee. Previously only a bare
// variable scrutinee was handled; a non-variable scrutinee now evaluates into r0 first via
// evaluate_general (which defers any scrutinee it cannot lower).
int mask(int n)        { switch(n & 3){ case 0: return 1; case 1: return 2; case 2: return 3; default: return 4; } }
int add1(int n)        { switch(n + 1){ case 0: return 1; case 1: return 2; default: return 0; } }
int andv(int a, int b) { switch(a & b){ case 0: return 1; case 1: return 2; default: return 0; } }
int shr(int n)         { switch(n >> 2){ case 0: return 1; case 1: return 2; default: return 0; } }
