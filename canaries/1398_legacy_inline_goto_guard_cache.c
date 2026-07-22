// builds: GC/1.2.5n
// flags: -Cpp_exceptions off

typedef struct Limits {
    float upper;
    int first_timer;
    float lower;
    int second_timer;
} Limits;

typedef struct State {
    float input;
    unsigned char timer;
    void* child;
} State;

extern Limits* limits;
extern int enabled(void*);
extern int check(void*);
extern void apply(void*, int);
extern void finish(void*);

static inline void scan_guards(State* state)
{
    if (state->input >= limits->upper && state->timer < limits->first_timer) {
        state->timer = 254;
        apply(state->child, 1);
        return;
    }
    if (state->input <= limits->lower &&
        state->timer < limits->second_timer && check(state->child))
    {
        state->timer = 254;
        apply(state->child, -1);
        finish(state->child);
    }
}

void legacy_inline_goto_guard_cache(State* state)
{
    if (enabled(state->child)) {
        scan_guards(state);
    }
}
