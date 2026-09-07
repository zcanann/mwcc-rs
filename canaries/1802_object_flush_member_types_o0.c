// Pointer, aggregate, narrowing, and overlapping clear boundaries.
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

extern State object;
typedef union Combined { unsigned value; unsigned short clear; } Combined;
extern Combined* combined;
void aggregate(void) { PORT.byte=97; PORT.word=object.value; object.clear=0; }
void narrowed(void) { PORT.byte=97; PORT.word=(unsigned short)state->value; state->clear=0; }
void overlap(void) { PORT.byte=97; PORT.word=combined->value; combined->clear=0; }
