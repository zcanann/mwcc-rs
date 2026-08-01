// builds: GC/1.3.2 GC/2.6
// flags: -O0,p -Cpp_exceptions off -sdata 0 -sdata2 0 -pool off

typedef struct State {
    float phase;
} State;

void advance_phase(State* state)
{
    state->phase += 9.0f;
}
