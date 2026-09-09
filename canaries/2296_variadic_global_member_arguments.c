struct State { unsigned unused[10]; unsigned memory; };
extern struct State *state;
extern void report(const char*, ...);
void memory_report(void) { report("Memory %u MB", state->memory >> 20); }
void combined_report(unsigned x) { report("Memory %u %u", x, state->memory >> 20); }
