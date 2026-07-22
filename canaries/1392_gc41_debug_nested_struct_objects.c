// builds: GC/3.0a3p1
// flags: -nodefaults -proc gekko -O4,s -inline noauto -sym on -schedule off -pragma "cats off"

struct Gc41DebugInner {
    int value;
};

struct Gc41DebugOuter {
    struct Gc41DebugInner inner;
    unsigned short tail;
};

struct Gc41DebugInner gc41_debug_inner_object;
struct Gc41DebugOuter gc41_debug_outer_object;
