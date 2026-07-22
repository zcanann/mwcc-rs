// builds: GC/3.0a3p1
// flags: -nodefaults -proc gekko -O4,s -inline noauto -sym on -schedule off -pragma "cats off"

void gc41_debug_ordinary_leaf(void) {}

__declspec(weak) void gc41_debug_weak_leaf(void) {}
