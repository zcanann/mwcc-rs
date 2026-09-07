// Declarations surround string-owning functions and survive the function tail.
// flags: -Cpp_exceptions off -pragma "cats off"
extern int observe(const char*);
extern void done(void);
static int before(void) { return observe("before"); }
static int pending;
static int after(void) { observe("after"); return pending; }
static int tail_zero;
static int tail_initialized = 11;
static void* const tail_reference = (void*)done;
