// Full BSS discovery can precede a function whose small-data uses follow it.
// flags: -Cpp_exceptions off -pragma "cats off"
extern int observe(const char*);
static int words[16];
static int small;
static int before(void) { return 7; }
static int read_words(void) { observe("packet"); return words[0]; }
static int read_small(void) { return small; }
static int unused_words[16];
