// builds: GC/1.2.5n
// flags: -Cpp_exceptions off

typedef struct Object {
    void* user_data;
} Object;

typedef struct State {
    Object* child;
    void (*callback)(void);
    unsigned char first : 1;
    unsigned char second : 1;
    unsigned char third : 1;
} State;

static inline void* user_data(Object* object)
{
    return object->user_data;
}

extern void prepare(State*, State*);
extern void change(Object*, int, int, float, float, float, void*);
extern void update(Object*);
extern void callback(void);
extern void collide(Object*, int);
extern void finish(State*);
extern void finish_with_value(State*, int);

void legacy_inline_post_call_schedule(Object* object)
{
    State* state = (State*) user_data(object);
    State* child = (State*) user_data(state->child);
    state->third = 0;
    prepare(child, state);
    change(object, 289, 144, 0, 1, 0, 0);
    update(object);
    state->callback = callback;
    state->first = 1;
    collide(object, 2);
    finish(state);
    finish_with_value(state, 511);
}
