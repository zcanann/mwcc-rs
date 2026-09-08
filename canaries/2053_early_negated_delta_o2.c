// flags: -Cpp_exceptions off -pragma "cats off" -O2
int input;
unsigned int unsigned_input;
volatile int observed;
extern void mutate(void);
void short_delta(short *out, int step) { input -= step * 160; *out = step * -1; }
void byte_delta(signed char *out, int step) { input -= step * 160; *out = step * -1; }
void triple_delta(short *out, int step) { input -= step * 3; *out = step * -1; }
void negative_factor(short *out, int step) { input -= step * -160; *out = step * -1; }
void unsigned_host(short *out, int step) { unsigned_input -= step * 160; *out = step * -1; }
void live_step(short *out, int step, int *after) { input -= step * 160; *out = step * -1; *after = step; }
void changed_step(short *out, int step) { input -= step * 160; step = 7; *out = step * -1; }
void volatile_host(short *out, int step) { observed -= step * 160; *out = step * -1; }
void called(short *out, int step) { input -= step * 160; mutate(); *out = step * -1; }
void computed_step(short *out, int value) { int step; step = value + 1; input -= step * 160; *out = step * -1; }
void local_live(short *out, int value, int *after) { int step; step = value + 1; input -= step * 160; *out = step * -1; *after = step; }
void clamped_step(int *volume, short *out) { int step; step = input / 160; if (step) { if (step > 20) step = 20; if (step < -20) step = -20; *volume = input; input -= step * 160; *out = step * -1; } else { *volume = 0; *out = 0; } }
void preceding_store(int *volume, int step, short *out) { *volume = input; input -= step * 160; *out = step * -1; }
