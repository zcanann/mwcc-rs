// flags: -Cpp_exceptions off -pragma "cats off"
struct State { unsigned short count_not, sent, count, padding; unsigned char save, abort_wait; unsigned dirty; };
void store_run(struct State* state, int token, unsigned value) {
    switch (token) {
    case -1: state->count = value; state->sent = 3; break;
    case 0: state->count = value >> 16; state->sent = 5; break;
    case 1: state->dirty = value; if (state->count) state->dirty ^= 32; break;
    default: state->save = 0; state->abort_wait = 1; break;
    }
}
void default_guard(struct State* state, int token, unsigned value) {
    switch (token) {
    case 1: state->count = 9; break;
    default:
        state->count = value;
        if (!state->count) state->save = 1;
        else state->abort_wait = 1;
        break;
    }
}
void empty_paths(struct State* state, int token, unsigned value) {
    switch (token) {
    case 0: break;
    case 1: state->dirty = value; break;
    case 2: break;
    }
}
