// First use belongs to its function; unused definitions finish at the tail.
// flags: -Cpp_exceptions off -pragma "cats off"
static int first;
static int second;
static int unused_a;
static int unused_b;
static int before(void) { return 1; }
static int read_second(void) { return second; }
static int gap(void) { return 4; }
static int read_first(void) { return first; }
