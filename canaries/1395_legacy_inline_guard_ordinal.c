// builds: GC/1.2.5n
// flags: -Cpp_exceptions off

typedef struct Limits {
    float upper;
    int first_timer;
    float lower;
    float second_timer;
} Limits;

typedef struct State {
    float input;
    unsigned char timer;
    void* child;
} State;

extern Limits* limits;
extern int check(void*);
extern void apply(void*, int);
extern void finish(void*);

static inline void step(State* state, int direction)
{
    state->timer = 254;
    apply(state->child, direction);
}

void legacy_inline_guard_ordinal(State* state)
{
    if (state->input >= limits->upper && state->timer < limits->first_timer) {
        step(state, 1);
        return;
    }
    if (state->input <= limits->lower &&
        state->timer < limits->second_timer && check(state->child))
    {
        step(state, -1);
        finish(state->child);
    }
}
