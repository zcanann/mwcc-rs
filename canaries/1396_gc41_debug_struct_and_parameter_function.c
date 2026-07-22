// builds: GC/3.0a3p1
// flags: -nodefaults -proc gekko -O4,s -inline noauto -sym on -schedule off -pragma "cats off"

struct Gc41DebugMixedParameterData {
    int value;
};

struct Gc41DebugMixedParameterData gc41_debug_mixed_parameter_data;

extern int gc41_debug_parameter_source(void);

void gc41_debug_mixed_parameter_function(int* destination) {
    *destination = gc41_debug_parameter_source();
}
