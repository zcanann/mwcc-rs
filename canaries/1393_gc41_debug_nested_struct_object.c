// builds: GC/3.0a3p1
// flags: -nodefaults -proc gekko -O4,s -inline noauto -sym on -schedule off -pragma "cats off"

struct Gc41DebugNestedInner {
    int value;
};

struct Gc41DebugNestedOuter {
    struct Gc41DebugNestedInner inner;
    unsigned short tail;
};

struct Gc41DebugNestedOuter gc41_debug_nested_outer_object;
