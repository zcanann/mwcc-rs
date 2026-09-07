// Zero declarations keep their source positions around body-owned strings.
// flags: -Cpp_exceptions off -pragma "cats off"
extern int observe(const char*);

int before() { return observe("before"); }

static int first;
static int explicit_zero = 0;

int after() {
    observe("after");
    return first;
}

static int tail;
