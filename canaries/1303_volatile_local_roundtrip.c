// Volatile automatic objects live in the stack frame and are reloaded rather
// than replaced by the last value assigned to them.
// builds: GC/1.3.2
// flags: -O4,p -inline auto

int volatile_roundtrip(int value) {
    volatile int current;
    current = value;
    return current;
}
