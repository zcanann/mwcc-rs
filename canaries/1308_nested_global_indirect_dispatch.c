// builds: 1.3.2 2.0 2.0p1 2.5 2.6 2.7
// flags: -Cpp_exceptions off -sdata 0 -sdata2 0 -O4,s

typedef short s16;
typedef unsigned short u16;

typedef struct Vec {
    float x;
    float y;
    float z;
} Vec;

typedef void (*Dispatch)(int, Vec, void *, void *, s16 *, u16, int, s16, s16);

typedef struct DispatchTable {
    int reserved[10];
    Dispatch dispatch;
} DispatchTable;

typedef struct GlobalState {
    char reserved[155804];
    DispatchTable *table;
} GlobalState;

extern GlobalState global_state;

void nested_global_dispatch(
    Vec value,
    int priority,
    s16 angle,
    void *context,
    u16 item,
    s16 argument0,
    s16 argument1
) {
    global_state.table->dispatch(
        85,
        value,
        0,
        context,
        &angle,
        item,
        priority,
        argument0,
        argument1
    );
}
