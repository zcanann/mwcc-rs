// Dolphin OSTime transactions: scalar interrupt state and wide arithmetic lifetimes.
typedef long long s64;
extern int disable_interrupts(void);
extern int restore_interrupts(int enabled);
extern s64 read_time(void);
s64 adjusted_now(void) {
    int enabled;
    s64* adjustment = (s64*)0x800030d8;
    s64 result;
    enabled = disable_interrupts();
    result = *adjustment + read_time();
    restore_interrupts(enabled);
    return result;
}
s64 adjusted_input(s64 time) {
    int enabled;
    s64* adjustment = (s64*)0x800030d8;
    s64 result;
    enabled = disable_interrupts();
    result = *adjustment + time;
    restore_interrupts(enabled);
    return result;
}
