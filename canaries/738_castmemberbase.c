// Member access on a CAST base: `((struct S *)x)->field`. The struct tag from
// the cast's target type is threaded through the wrapping parens (the cast is
// parsed in a nested factor), and the pointer cast is transparent at codegen —
// the base is the operand's pointer value. Loads and stores; chained members;
// struct-typedef casts.
struct Cmb { int first; int second; struct Cmb *next; };
int cmb_load(void *x)            { return ((struct Cmb *)x)->second; }
void cmb_store(void *x, int v)   { ((struct Cmb *)x)->first = v; }
int cmb_chain(void *x)           { return ((struct Cmb *)x)->next->first; }
