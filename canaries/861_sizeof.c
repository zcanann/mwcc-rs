// sizeof(type) is a compile-time constant (size_t) that lowers to `li r3, N`: a struct uses its
// laid-out (padded) size, a pointer is 4, and a scalar is its byte width. Typedef names resolve
// through the type table. It folds into surrounding constant expressions. `sizeof` of an
// EXPRESSION (`sizeof x`, `sizeof(expr)`) needs the operand's type and still defers. Ubiquitous
// in real code (memcpy/memset sizes, array element counts).
int sz_int(void)     { return sizeof(int); }        // 4
int sz_char(void)    { return sizeof(char); }       // 1
int sz_short(void)   { return sizeof(short); }      // 2
int sz_uint(void)    { return sizeof(unsigned int); }// 4
int sz_double(void)  { return sizeof(double); }     // 8
int sz_ptr(void)     { return sizeof(int *); }      // 4
struct S { char a; int b; };
int sz_struct(void)  { return sizeof(struct S); }   // 8 (padded)
int sz_expr(void)    { return sizeof(int) * 4; }    // 16 (const-folded)
