// flags: -Cpp_exceptions off -pragma "cats off"
extern long long clock_value(void);
extern unsigned counter_value(void);
static inline void wait_ticks(void) {
    long long start;
    long long now;
    start = clock_value();
    do { now = clock_value(); } while (now - start <= 48 / 4);
}
static inline void settle(void) {
    unsigned previous;
    unsigned current;
    previous = counter_value();
    do {
        current = previous;
        wait_ticks();
        previous = counter_value();
    } while (previous != current);
}
void wait_twice(unsigned mode) {
    wait_ticks();
    if (mode) wait_ticks();
}
void guarded_settle(unsigned enabled) {
    if (enabled) settle();
}
