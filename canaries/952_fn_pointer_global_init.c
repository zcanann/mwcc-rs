// A file-scope function-pointer global may carry an initializer, like any pointer global.
// `= 0` is a NULL pointer: an EXPLICIT zero that lands in `.sbss` (ordered ahead of the
// uninitialized run, see 951). `= func` / `= &func` is the address of a function: an ADDR32
// relocation to that symbol in `.sdata`. Previously the fn-pointer declarator path required
// a `;` immediately after the signature and failed on the `=` (a common exit/callback-table
// shape, e.g. `void (*__stdio_exit)(void) = 0;`).
//
// DEFERS (no wrong bytes): a `static` function-pointer with an address initializer (the
// static/const pointer-address global is a separate roadmap item).
extern void handler(void);

void (*on_exit_fn)(void)    = 0;         // NULL  -> .sbss (explicit zero)
void (*on_abort_fn)(void)   = 0;         // NULL  -> .sbss (explicit zero)
void (*dispatch_fn)(void)   = handler;   // &func -> .sdata + ADDR32 reloc
void (*pending_fn)(void);                // uninitialized -> .sbss (reversed run)

int poke(void) { return dispatch_fn != 0; }
