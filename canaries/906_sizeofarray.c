// `sizeof` of a local array: the WHOLE array folds to element_size*length; an ELEMENT (`arr[i]` /
// `*arr`) folds to the element size; and the array-length idiom `sizeof(arr)/sizeof(arr[0])` folds
// to the count. The parser records the element type (variable_types) and the array total
// (variable_array_bytes, cleared per function). A declared-but-only-sizeof'd array elides (no frame
// slot), so these are leaf `li r3,N`. (Using the array's storage still defers on local arrays, #19.)
int   int4_total(void)       { int b[4];      return sizeof(b); }                 // 16
int   char10_total(void)     { char b[10];    return sizeof(b); }                 // 10
int   double3_total(void)    { double b[3];   return sizeof(b); }                 // 24
int   elem_word(void)        { int b[4];      return sizeof(b[0]); }              // 4
int   elem_deref(void)       { double b[3];   return sizeof(*b); }               // 8
int   int4_count(void)       { int b[4];      return sizeof(b)/sizeof(b[0]); }    // 4
int   char10_count(void)     { char b[10];    return sizeof(b)/sizeof(b[0]); }    // 10
struct S { int x, y; };
int   struct3_total(void)    { struct S b[3]; return sizeof(b); }                 // 24
int   struct3_count(void)    { struct S b[3]; return sizeof(b)/sizeof(b[0]); }    // 3
