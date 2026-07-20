// A parameter read only in an if-arm still crosses a call made by the condition.
// The survivor is a virtual value colored to r31 by the callee-saved allocator;
// the CFG owner supplies only the branch and standard one-save frame boundaries.
// builds: GC/1.3.2
// flags: -Cpp_exceptions off -O0,p -char unsigned -sdata 0 -sdata2 0 -pool off
typedef unsigned char u8;
typedef void (*Callback)(void);

typedef struct Object {
    int prefix[5];
    Callback callback;
} Object;

extern u8 status(void);
extern void finish(int first, int second);

void call_condition_live_in(Object* object) {
    if (status() == 0) {
        object->callback = 0;
        finish(1, 1);
    }
}
