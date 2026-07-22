// builds: GC/1.2.5n
// flags: -Cpp_exceptions off

typedef struct State {
    void* victim;
    int timer;
} State;

typedef struct Object {
    int scale;
} Object;

typedef struct Entry {
    Object* rendered;
    State* state;
} Entry;

extern int should_update(void*, State*);
extern void use_entry(Entry*);
extern void use_state(State*);
extern void use_object(Object*);
extern void finish(void*, int);

void legacy_batched_saved_prologue(Entry* object)
{
    State* state = object->state;
    Object* rendered = object->rendered;

    if (should_update(state->victim, state)) {
        use_entry(object);
    }

    state->timer -= 1;
    if (state->timer <= 0) {
        void* victim = state->victim;
        use_state(state);
        finish(victim, 1);
        use_entry(object);
        use_object(rendered);
        finish(victim, 0);
    }
}
