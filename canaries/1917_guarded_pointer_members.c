// flags: -Cpp_exceptions off -pragma "cats off"
// A global struct pointer's address and its member values have distinct caches.
struct State { unsigned value; unsigned flags; unsigned dirty; };
extern struct State *state;
#define PORT (*(volatile unsigned *)0xcc008000)
void update(unsigned value) { unsigned old = state->value; state->value = value; if (old != state->value) { PORT = state->value; state->flags |= 512; state->dirty |= 4; } }
void conditional(unsigned value) { if (state->value != value) { PORT = state->value; state->value = value; } PORT = state->value; }
void nested(unsigned value, unsigned flag) { if (state->value != value) { PORT = state->value; if (flag) state->flags |= 512; state->dirty |= 4; } PORT = state->flags; }
