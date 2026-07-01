// A whole-body `if (c) { <run> } else { <run> }` where each arm is two-plus stores whose values
// are all REGISTER-valued (params/locals already in registers) — emitted sequentially, no value
// to materialize — or all CONSTANT (batched). Either kind per arm, mixed across arms:
//   cmpwi; beq else; <then run>; blr; else: <else run>; blr
// Extends 964 (both-constant arms) to register-valued and mixed reg/const arms.
struct Vec2 { int x, y; };
int g, h;

void both_reg(int a, int b, int c)   { if (a) { g = b; h = c; } else { g = c; h = b; } }
void reg_then_const(int a, int b)    { if (a) { g = b; h = b; } else { g = 1; h = 2; } }
void const_then_reg(int a, int b)    { if (a) { g = 1; h = 2; } else { g = b; h = b; } }
void members(struct Vec2 *p, int a, int u, int v) { if (a) { p->x = u; p->y = v; } else { p->x = v; p->y = u; } }
