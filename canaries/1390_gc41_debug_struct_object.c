// builds: GC/3.0a3p1
// flags: -nodefaults -proc gekko -O4,s -inline noauto -sym on -schedule off -pragma "cats off"

struct Gc41DebugPairObject {
    int first;
    unsigned short second;
};

struct Gc41DebugPairObject gc41_debug_pair_object;
