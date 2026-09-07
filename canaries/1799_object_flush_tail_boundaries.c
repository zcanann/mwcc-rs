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

void volatile_pointer(void) { PORT.byte=97; PORT.word=changing->value; changing->dirty|=3; changing->clear=0; }
void different_dirty(void) { PORT.byte=97; PORT.word=state->value; other->dirty|=3; state->clear=0; }
void high_mask(void) { PORT.byte=97; PORT.word=state->value; state->dirty|=0x12340000; state->clear=0; }
void explicit_cast(void) { ((volatile Pipe*)0xCC008004)->byte=97; ((volatile Pipe*)0xCC008004)->word=state->value; state->dirty|=3; state->clear=0; }

void volatile_plain(void) { PORT.byte=97; PORT.word=changing->value; changing->clear=0; }
