// File statics retain declaration frontiers even when first uses are later.
// flags: -Cpp_exceptions off -pragma "cats off"
static int before(void) { return 1; }
static int first;
static int second;
static int explicit_zero = 0;
static int initialized = 7;
static int bulk[16];
static int gap(void) { return 4; }
static int read_second(void) { return second; }
static int read_first(void) { return first; }
static int unreferenced;
int entry(void) { return 0; }
