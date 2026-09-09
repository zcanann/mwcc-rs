/* Global condition values must be retained or reloaded at call boundaries. */
unsigned limit;
unsigned end;
volatile unsigned status;
extern unsigned query(void);
extern void consume(unsigned, unsigned, unsigned);
extern void mutate(void);
void guarded_value(void) {
    if (limit) consume(limit, 0, 0);
}
void queried_condition(void) {
    if (query() < limit) consume(limit, 0, 0);
}
void queried_argument(void) {
    if (limit) consume(limit, query(), limit);
}
void changed_body(void) {
    if (limit) { mutate(); consume(limit, 0, 0); }
}
void nested_conditions(void) {
    if (query() < limit) {
        if (query() <= limit) { consume(limit, 1, 0); return; }
        consume(limit, query(), 2);
        if (query() > end) consume(end, query() - end, 3);
    }
}
void volatile_body(void) {
    if (status) consume(status, query(), status);
}
void aliased_body(unsigned *out) {
    if (limit) { *out = 0; consume(limit, 0, 0); }
}
