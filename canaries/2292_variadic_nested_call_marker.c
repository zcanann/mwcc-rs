/* GC 4.1 omits the CR1 variadic marker only with nested argument calls. */
extern void report(const char*, ...);
extern unsigned query(void);

void word(void) { report("w", 1); }
void floating(double d) { report("d", d); }
void nested(void) { report("n", query()); }
void computed(void) { report("c", query() + 1); }
