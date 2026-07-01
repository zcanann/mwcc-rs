// A trailing-void, no-else if-BLOCK of two stores whose values are COMPUTED (single-op) and/or a
// mix of computed / leaf / constant reuses mwcc's latency-scheduled value overlap, wrapped in a
// conditional return: `<test>; b<!c>lr; <overlap the two values>; <two stores>`. Same scheduler
// that already lowers the straight-line two-store run (try_computed_store_fill / try_mixed_store_fill),
// extended to the if-wrapped form.
//   if(a){g=a+1;h=b+2;} -> cmpwi;beqlr; addi r3,r3,1; addi r0,r4,2; stw r3,g; stw r0,h
//   if(a){g=a+1;h=b;}   -> cmpwi;beqlr; addi r0,r3,1; stw r4,h; stw r0,g   (leaf stored first)
int g, h;

void both_computed(int a, int b) { if (a)     { g = a + 1; h = b + 2; } }
void shift_and_sub(int a, int b) { if (a)     { g = a << 2; h = b - 1; } }
void computed_const(int a, int b){ if (a)     { g = a + 1; h = 5; } }
void computed_leaf(int a, int b) { if (a)     { g = a + 1; h = b; } }        // mixed
void leaf_const(int a, int b)    { if (a)     { g = a; h = 5; } }            // mixed
void with_compare(int a, int b)  { if (a > b) { g = a * b; h = a + b; } }
