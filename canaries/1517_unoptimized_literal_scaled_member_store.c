// builds: GC/1.3.2 GC/2.6
// flags: -O0,p -Cpp_exceptions off -fp_contract off -sdata 0 -sdata2 0 -pool off

typedef struct State {
    float input;
    float output;
} State;

void place_scaled_value(State* state, float scale)
{
    state->output = 0.001f + state->input * scale;
}
