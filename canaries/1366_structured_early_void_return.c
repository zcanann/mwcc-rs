// builds: GC/1.2.5n

typedef struct State {
    int first;
    int second;
} State;

void first_action(State*);
void second_action(State*);

void structured_early_void_return(State* state)
{
    if (state->first) {
        first_action(state);
        return;
    }
    if (state->second) {
        second_action(state);
    }
}
