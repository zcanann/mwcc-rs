// builds: GC/1.3.2 GC/2.6
// flags: -O0,p -Cpp_exceptions off -fp_contract off -sdata 0 -sdata2 0 -pool off

typedef struct State {
    float input;
    float output;
} State;

void reflect_value(State* state)
{
    state->output = 2.5f - state->input;
}
