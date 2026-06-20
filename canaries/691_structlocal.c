// A frame-resident struct-value local: `struct S v;` gets a stack slot of the
// struct's size (aligned to its member alignment), and `&v` is its address. The
// first struct-value support.
struct B { int x; int y; int z; };
void g(struct B *);
void structlocal(void){ struct B v; g(&v); }
