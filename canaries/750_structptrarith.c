// Pointer arithmetic on a struct pointer scales by the struct's byte size, not 1.
// A constant index folds the scaled offset into an `addi`; a variable index scales
// with `slwi` for a power-of-two struct size and `mulli` otherwise (e.g. *12). The
// element size now travels with `Type::StructPointer`, threaded into the operand's
// stride. A function pointer (size 0) is left unscaled.
struct One  { int a; };              /* 4 bytes  */
struct Tri  { int a, b, c; };        /* 12 bytes */
struct Duo  { int a, b; };           /* 8 bytes  */
struct One *one_const(struct One *p)         { return p + 1; }
struct Tri *tri_const(struct Tri *p)         { return p + 1; }
struct Tri *tri_var(struct Tri *p, int i)    { return p + i; }
struct One *one_var(struct One *p, int i)    { return p + i; }
struct Duo *duo_pow2(struct Duo *p)          { return p + 5; }
struct Tri *tri_back(struct Tri *p, int i)   { return p - i; }
struct Tri *tri_back1(struct Tri *p)         { return p - 1; }
