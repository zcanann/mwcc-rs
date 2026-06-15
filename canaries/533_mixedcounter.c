// const, leaf, const: exercises the cross-function @N counter — a leaf function
// in the middle still advances the counter, so the third function's constant is
// numbered @14 (5 + 1 + 4, then + 0 + 4), not @6.
float cfa(float x){ return x * 2.0f; }
int   cfb(int a){ return a; }
float cfc(float x){ return x * 3.0f; }
