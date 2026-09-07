// flags: -Cpp_exceptions off -pragma "cats off"
extern long long clock_read(void);
extern long long last_clock;
int elapsed(void) { long long current; current = clock_read(); if (current - last_clock < 10) return 0; last_clock = current; return 1; }
int initialized(void) { long long current = clock_read(); return current < last_clock; }
int equal_clock(void) { long long current; current = clock_read(); return current == last_clock; }
int unsigned_elapsed(void) { unsigned long long current; current = clock_read(); return current - (unsigned long long)last_clock < 10; }
int update_sum(void) { long long current; current = clock_read(); current = current + last_clock; last_clock = current; return (int)current; }
extern unsigned before_clock(void);
extern void after_clock(unsigned);
int ordered(void) { unsigned token = before_clock(); long long current = clock_read(); after_clock(token); return current < last_clock; }
