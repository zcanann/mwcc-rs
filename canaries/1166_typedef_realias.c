// Typedef RE-ALIASING preserves struct identity (fire 675). `typedef Existing NewAlias;` copies
// the original struct/struct-pointer/array registration — parse_type's scalar model would lose it
// (the old behavior deferred `T2* t` with "pointer to Struct is not supported"). The motivating
// shape is stdarg's `typedef struct {…} __va_list[1]; typedef __va_list va_list;` — an ARRAY-of-
// struct typedef re-aliased, where a va_list parameter decays to the struct pointer (this flipped
// melee __va_arg.c's defer from the parser into codegen). The tagged-typedef path also now consumes
// the `[N]` declarator suffix cleanly instead of relying on error recovery.
typedef struct { int x; int y; } TRS;
typedef TRS TRS2;
int tra(TRS2* t) { return t->y; }
typedef struct { char gpr; char fpr; char reserved[2]; char* input_arg_area; char* reg_save_area; } __va_list_c[1];
typedef __va_list_c va_list_c;
int trb(va_list_c v) { return v->gpr; }
char* trc(va_list_c v) { return v->input_arg_area; }
typedef struct VaLike { int a; char* p; } VaArr[1];
typedef VaArr VaArr2;
char* trd(VaArr2 v) { return v->p; }
