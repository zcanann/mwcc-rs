// A local initialized from a global is a value snapshot across a later write.
// builds: GC/1.2.5n
// flags: -Cpp_exceptions off -O4,p -inline all -sdata 8 -sdata2 8
unsigned short saved_global = 0xffff;

unsigned short exchange_saved_global(unsigned short replacement) {
    unsigned short old;
    old = saved_global;
    saved_global = replacement;
    return old;
}

unsigned short read_saved_global(void) {
    return saved_global;
}
