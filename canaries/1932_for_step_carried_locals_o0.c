// flags: -Cpp_exceptions off -pragma "cats off"
// GXGeometry's flush loop has its only induction write in the for step.
struct State { unsigned short count, stride, sent; };
extern struct State *state;
void flush(void) { unsigned i; unsigned n = state->count * state->stride; *(volatile unsigned char *)0xcc008000 = 0x98; *(volatile unsigned short *)0xcc008000 = state->count; for (i = 0; i < n; i += 4) { *(volatile unsigned *)0xcc008000 = 0; } state->sent = 1; }
unsigned step_sum(unsigned n) { unsigned i, sum = 0; for (i = 0; i < n; i += 3) { sum += i; } return sum; }
void step_store(unsigned n) { unsigned i; for (i = 0; i < n; i += 4) { *(volatile unsigned *)0xcc008000 = i; } }
unsigned postfix(unsigned n) { unsigned i; for (i = 0; i < n; i++) { *(volatile unsigned *)0xcc008000 = i; } return i; }
unsigned continued(unsigned n) { unsigned i, sum = 0; for (i = 0; i < n; i += 2) { if (i & 2) continue; sum += i; } return sum; }
void macro_flush(void) { unsigned i; unsigned n = state->count * state->stride; *(volatile unsigned char *)0xcc008000 = 0x98; *(volatile unsigned short *)0xcc008000 = (unsigned short)state->count; for (i = 0; i < n; i += 4) { *(volatile unsigned *)0xcc008000 = 0; } state->sent = 1; }
