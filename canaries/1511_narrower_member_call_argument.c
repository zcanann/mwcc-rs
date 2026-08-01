// builds: GC/1.3.2
// flags: -O0,p -Cpp_exceptions off -sdata 0 -sdata2 0 -pool off

typedef signed short s16;
typedef unsigned char u8;

typedef struct State {
    s16 first;
    s16 second;
    s16 selector;
} State;

extern State state;
extern void consume(s16, s16, u8, u8);

void narrower_member_call_argument(void)
{
    consume(state.first, state.second, state.selector, 17);
}
