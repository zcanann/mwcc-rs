// A NON-trailing multi-store if-BLOCK that is the FIRST statement of a void body, followed by
// exactly one trailing store: `cmpwi; beq cont; <then run>; cont: <trailing store>; blr`. The
// false path forward-branches over the then-run to the continuation the loop emits next. The
// if-first restriction avoids the leading-store-before-if scheduler (which still defers), and the
// single trailing store is what the loop emits byte-exactly. The then-run may be constant (batched)
// or register-valued (sequential).
struct Vec2 { int x, y; };
int g, h, k;

void const_run(int a)              { if (a) { g = 1; h = 2; } k = 3; }
void reg_run(int a, int b)         { if (a) { g = b; h = b; } k = b; }
void const_run_reg_tail(int a, int b) { if (a) { g = 1; h = 2; } k = b; }
void members(struct Vec2 *p, int a, int v) { if (a) { p->x = v; p->y = v; } k = v; }
