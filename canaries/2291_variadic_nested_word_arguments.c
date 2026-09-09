/* String addresses are word-valued inputs to variadic call transactions. */
extern unsigned query(void);
extern void *pointer_query(void);
extern void report(const char*, ...);
void pair_report(void) { report("%u %u", query(), query()); }
void computed_report(unsigned x) { report("value %u %u", query() + x, query()); }
void pointer_report(void) { report("pointer %p %u", pointer_query(), query()); }
void conditional_report(void) { if (query()) report("guard %u %u", query(), query()); }
