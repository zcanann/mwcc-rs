// Absolute-object port scheduling and state-pointer reuse.
// flags: -O0 -Cpp_exceptions off -pragma "cats off"
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

void plain(void) { PORT.byte=97; PORT.word=state->value; state->clear=0; }
void dirty(void) { PORT.byte=97; PORT.word=state->value; state->dirty|=3; state->clear=0; }
void alias_data(void) { PORT.byte=42; PORT.word=state->value; state->value|=0x8001; state->clear=0; }
