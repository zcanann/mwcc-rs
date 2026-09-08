// flags: -Cpp_exceptions off -pragma "cats off"
// Declaration order and inline invocation scopes around the GC 1.1p1 O0 spill.
extern long long clock_value(void);
extern unsigned counter_value(void);

static inline void short_wait(void) {
    long long start, now;
    start = clock_value();
    do {
        now = clock_value();
    } while (now - start <= 7);
}

static inline void reversed_wait(void) {
    long long now, start;
    start = clock_value();
    do {
        now = clock_value();
    } while (now - start <= 19);
}

void once(unsigned enabled) {
    if (enabled) short_wait();
}

void signed_once(int enabled) {
    if (enabled) short_wait();
}

void reversed(unsigned enabled) {
    if (enabled) reversed_wait();
}

void stabilized(unsigned enabled) {
    unsigned current, previous;
    if (enabled) {
        previous = counter_value();
        do {
            current = previous;
            short_wait();
            previous = counter_value();
        } while (previous != current);
    }
}

void three(unsigned enabled) {
    short_wait();
    if (enabled) {
        short_wait();
        short_wait();
    }
}
