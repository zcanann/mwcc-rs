// builds: GC/1.1 GC/1.3 GC/1.3.2 GC/2.0 GC/2.5 GC/2.6 GC/2.7
// flags: -O4,p -inline auto -Cpp_exceptions off

extern void consume(int value);

void nonreturning_saved_loop(int value)
{
    while (1) {
        consume(value);
    }
}
