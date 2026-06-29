// A `T a[]` (or `T a[N]`) function PARAMETER is exactly `T* a` — C array-to-pointer parameter
// decay; the size, if written, is irrelevant. ours failed to parse the `[...]` after a
// parameter name ("expected ParenClose, found BracketOpen"), deferring the whole function.
// The parser now consumes the `[...]` and decays the parameter to a pointer to the element
// type (a scalar -> Pointer, a struct -> StructPointer), so these compile in full parity with
// the `T* a` forms.
int  load_index(int a[], int i)          { return a[i]; }        // = int* a : slwi; lwzx
int  load_const(int a[])                 { return a[0]; }        // lwz 0(a)
int  load_sized(int a[10], int i)        { return a[i]; }        // [10] ignored
int  load_char(char a[], int i)          { return a[i]; }        // lbzx; extsb (narrow)
void store_index(int a[], int i, int x)  { a[i] = x; }           // slwi; stwx
struct S { int x; };
int  member_array(struct S a[], int i)   { return a[i].x; }      // struct-array member access
