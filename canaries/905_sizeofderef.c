// `sizeof(expr)` for more resolvable forms, each folding to a `size_t` constant (`li r3,N`),
// building on the variable_types map: a pointer DEREF / SUBSCRIPT yields the pointee size
// (`*p` / `a[i]`), a struct MEMBER yields the member's size (`s->f`), and a CAST yields the
// target type's size. Other shapes (`sizeof(arr)` — needs the element count; arithmetic) still defer.
int    deref_int(int *p)              { return sizeof(*p); }     // 4
int    deref_char(char *p)            { return sizeof(*p); }     // 1
int    deref_double(double *p)        { return sizeof(*p); }     // 8
struct S { int x; char c; };
int    deref_struct(struct S *s)      { return sizeof(*s); }     // 8 (laid-out size)
int    member_word(struct S *s)       { return sizeof(s->x); }   // 4
int    member_byte(struct S *s)       { return sizeof(s->c); }   // 1
int    subscript(int *a)              { return sizeof(a[2]); }   // 4 (element size)
int    cast_char(int a)               { return sizeof((char)a); } // 1
