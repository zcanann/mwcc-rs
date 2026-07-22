// builds: GC/3.0a3p1
// flags: -nodefaults -proc gekko -O4,s -inline noauto -sym on -schedule off -pragma "cats off"

struct Gc41DebugMixedPair {
    int first;
    unsigned short second;
};

struct Gc41DebugMixedPair gc41_debug_mixed_pair;

int gc41_debug_mixed_function(void) {
    return 9;
}
