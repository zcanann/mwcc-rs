// An outer call whose FIRST argument is a nested argument-free call and whose SECOND argument is the
// single parameter, which must survive the nested call: `h(g(), p)`. mwcc saves the parameter in r31
// (`mr r31,p`), runs the nested call (result -> r3 = the outer call's first argument), materializes
// the saved parameter into the second argument register (`mr r4,r31`), then calls the outer function.
// This is MSL alloc.c's `free`: `__pool_free(get_malloc_pool(), ptr)`. (fire 592)
void* g(void);
int gi(void);
void h(void*, void*);
int hi(int, int);
void nested_void(void* p) { h(g(), p); }          // mr r31,r3; bl g; mr r4,r31; bl h
int nested_return(int p)  { return hi(gi(), p); }  // ...; hi's result already in r3
