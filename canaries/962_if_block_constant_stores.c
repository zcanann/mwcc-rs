// A trailing-void, no-else if-BLOCK of two-or-more CONSTANT stores reuses mwcc's batched
// constant materialization, wrapped in a conditional return: `<test>; b<!c>lr; <materialize all
// values into r(N+1)..r3,r0>; <stores in source order>`. This is the same scheduler that already
// lowers a straight-line constant store run — extended to the if-wrapped form.
//   if(a){g=1;h=2;}       -> cmpwi;beqlr; li r3,1; li r0,2; stw r3,g; stw r0,h
//   if(a){g=1;h=2;k=3;}   -> cmpwi;beqlr; li r4,1; li r3,2; li r0,3; stw r4; stw r3; stw r0
// A REGISTER value (961), a global/computed value, an else arm, or a call all take other paths.
int g, h, k, m;

void two(int a)     { if (a)      { g = 1; h = 2; } }
void three(int a)   { if (a)      { g = 1; h = 2; k = 3; } }
void four(int a)    { if (a)      { g = 1; h = 2; k = 3; m = 4; } }
void same(int a)    { if (a)      { g = 7; h = 7; } }          // one register reused
void cmp(int a, int b) { if (a > b) { g = 1; h = 2; } }        // a compare condition
