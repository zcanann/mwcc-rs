// builds: GC/3.0a3p1
// flags: -nodefaults -proc gekko -O4,s -inline noauto -sym on -schedule off -pragma "cats off"

struct Gc41DebugUnusedPair {
    int first;
    unsigned short second;
};

int gc41_debug_constant_function(void) {
    return 7;
}
