// A single local initialized from a MEMORY read, inlined into its use in a store-free
// leaf body -- `int t = arr[i]; return t + 1;` compiles exactly like the direct
// `return arr[i] + 1;` (`lis; slwi; addi; lwzx r3; addi r3,1`). Covers array elements,
// members, and dereferences. A twice-read load or an aliasing store defers (the load
// must not duplicate or reorder). Previously "a global-array subscript into the
// scratch register".
struct S { int a; int x; };
int arr[6];

int element_use(int i)       { int t = arr[i]; return t + 1; }
int element_ret(int i)       { int t = arr[i]; return t; }
int member_use(struct S *p)  { int t = p->x; return t * 2; }
int deref_use(int *p)        { int t = *p; return t + 3; }
