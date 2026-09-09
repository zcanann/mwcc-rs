/* Typed intermediate values survive nested calls until ABI placement. */
unsigned limit;
unsigned end;
volatile unsigned status;
extern unsigned query(void);
extern unsigned *pointer_query(void);
extern void consume(unsigned, unsigned, unsigned);
void call_difference(void) { consume(query(), 0, query() - query()); }
void call_sum(void) { consume(query(), query() + query(), query()); }
void retained_argument(unsigned x) { consume(x, query() + x, query()); }
void call_comparison(void) { consume(query() < query(), query(), 7); }
void signed_argument(void) { consume(query(), (unsigned)((int)query() / -3), query()); }
void shared_computed_global(void) { consume(end, query() - end, 3); }
void pointer_argument(void) { consume((unsigned)(pointer_query() + query()), 0, query()); }
void cast_pointer_argument(void) { consume((unsigned)pointer_query() + query(), 0, query()); }
