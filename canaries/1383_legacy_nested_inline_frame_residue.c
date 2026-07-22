// builds: GC/1.2.5n
// flags: -Cpp_exceptions off

typedef struct Object {
    void* user_data;
} Object;

typedef struct State {
    Object* child;
    unsigned char first : 1;
    unsigned char second : 1;
    unsigned char third : 1;
} State;

static inline void* user_data(Object* object)
{
    return object->user_data;
}

extern void prepare(State*, State*);
extern void consume(Object*, int, int, float, float, float, void*);
extern void finish(void*);

void legacy_nested_inline_frame_residue(Object* object)
{
    State* state = (State*) user_data(object);
    State* child = (State*) user_data(state->child);
    state->third = 0;
    prepare(child, state);
    consume(object, 289, 144, 0, 1, 0, 0);
    state->first = 1;
    finish(object);
    finish(state);
}
