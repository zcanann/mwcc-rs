// Optimized-away parameters do not receive formal-parameter DIEs. Deferred
// emission reverses both the line records and subprogram DIEs with the bodies.
// builds: GC/1.1
// flags: -Cpp_exceptions off -O4,p -inline deferred -sym on

int debug_text(const char* format, const char* file, int line, ...) { return 1; }

int debug_setup(void) { return 1; }

int debug_reset(void) { return 1; }
