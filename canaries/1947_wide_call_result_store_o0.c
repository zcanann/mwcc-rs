// flags: -Cpp_exceptions off -pragma "cats off"
extern long long clock_read(void);
extern long long last_clock;
void save_clock(void) { last_clock = clock_read(); }
void capture_clock(long long *out) { *out = clock_read(); }
