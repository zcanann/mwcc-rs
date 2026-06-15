struct S { int a; int b; };
int twomembersub(struct S* p, int x){ return (p->a - p->b) * x; }
