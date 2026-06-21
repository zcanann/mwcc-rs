// An indirect call through a function-pointer parameter: the pointer is copied to
// r12 before the arguments overwrite its register, then `mtctr r12; bctrl`, with the
// saved-LR store delayed past the setup moves (`mr r12,fp; mr r3,arg`) as mwcc does.
typedef int (*FnPtrI)(int);
typedef void (*FnPtrV)(void);
typedef int (*FnPtr2)(int, int);
int  fpc_one(FnPtrI f, int x)        { return f(x); }
void fpc_void(FnPtrV f)              { f(); }
int  fpc_two(FnPtr2 f, int a, int b) { return f(a, b); }
