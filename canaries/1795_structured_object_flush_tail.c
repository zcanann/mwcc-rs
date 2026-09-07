// Absolute-object port scheduling and state-pointer reuse.
// flags: -Cpp_exceptions off -pragma "cats off"
typedef struct State {
    unsigned short pad, clear;
    unsigned value, dirty, untouched;
} State;
typedef struct State* StatePointer;
extern State* state;
extern State* other;
extern volatile StatePointer changing;
typedef union Pipe { unsigned char byte; unsigned word; } Pipe;
volatile Pipe PORT : 0xCC008000;

void branch(int choice) {
    if (choice) state->value ^= 0x55; else state->value += 7;
    PORT.byte=97; PORT.word=state->value; state->dirty|=3; state->clear=0;
}
void early(int choice) {
    if (choice) return;
    PORT.byte=97; PORT.word=state->value; state->dirty|=3; state->clear=0;
}
void redirect(int choice) {
    if (choice) state=other;
    PORT.byte=97; PORT.word=state->value; state->dirty|=3; state->clear=0;
}
