// An empty variadic definition still owns the complete 112-byte EABI register
// save area: conditional f1..f8 saves followed by unconditional r3..r10 saves.
// builds: GC/1.2.5n
// flags: -Cpp_exceptions off
void empty_variadic(const char* format, ...) {
}
