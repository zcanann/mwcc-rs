// `sizeof(var)` / `sizeof var` for a parameter or scalar local of a known type folds to a `size_t`
// constant (`li r3,N`), exactly like `sizeof(type)`. The parser now tracks each param/scalar-local's
// declared type (variable_types, cleared per function); the sizeof handler resolves a plain-variable
// operand to its type's size (struct -> laid-out size, pointer -> 4, scalar -> width/8). Other expr
// shapes (`sizeof(*p)`, `sizeof(s->field)`, `sizeof(arr)`, arithmetic) still defer — NOT DIFF.
int   size_int(int a)            { return sizeof(a); }       // li r3,4
int   size_char(char a)          { return sizeof(a); }       // li r3,1
int   size_short(short a)        { return sizeof(a); }       // li r3,2
int   size_ptr(int *p)           { return sizeof(p); }       // li r3,4
int   size_double(double a)      { return sizeof(a); }       // li r3,8
int   size_local(int a)          { int b; return sizeof(b); }
int   size_noparen(int a)        { return sizeof a; }
int   size_sum(int a, short b)   { return sizeof(a) + sizeof(b); }  // 4 + 2
struct S { int x, y; };
int   size_struct_val(struct S s){ return sizeof(s); }       // li r3,8
