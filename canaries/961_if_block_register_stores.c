// A trailing-void, no-else if-BLOCK of two-or-more REGISTER-VALUED stores lowers straight:
// `<test>; b<!c>lr; <stores in source order>`. Each store's value is a parameter/local already
// in a register, so mwcc emits the stores sequentially after the conditional return — nothing to
// materialize or schedule. (A constant/global/computed store value instead needs mwcc's batched
// value-materialization scheduler and still defers.)
struct Vec2 { int x, y; };
int g, h, k;

void set_globals(int a, int b)              { if (a) { g = b; h = b; } }        // stw b,g; stw b,h
void set_two_values(int a, int b, int c)    { if (a) { g = b; h = c; } }        // stw b,g; stw c,h
void set_members(struct Vec2 *p, int a, int v) { if (a) { p->x = v; p->y = v; } } // stw v,0(p); stw v,4(p)
void set_three(int a, int b)                { if (a) { g = b; h = b; k = b; } } // three stores
