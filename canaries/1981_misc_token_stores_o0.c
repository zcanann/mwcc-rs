// flags: -Cpp_exceptions off -pragma "cats off"
struct State { unsigned short count_not, sent, count, padding; unsigned char save, abort_wait; unsigned dirty; };
extern struct State* state;
void set_misc(int token, unsigned value) {
    switch (token) {
    case 1:
        state->count = value;
        state->count_not = !state->count;
        state->sent = 1;
        if (state->count != 0) state->dirty |= 8;
        break;
    case 2: state->save = value != 0; break;
    case 3: state->abort_wait = value != 0; break;
    case 0: break;
    default: break;
    }
}
