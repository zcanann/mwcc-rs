// builds: GC/1.3 GC/1.3.2 GC/2.0 GC/2.0p1 GC/2.5 GC/2.6 GC/2.7
#pragma cplusplus on

typedef int (*Callback)(void*, void*);

extern int invoke(short kind, Callback callback, void* context);

int call_invoke(void) {
    return invoke(1, 0, 0);
}

#pragma cplusplus off
