// Named parameters enlarge the variadic save frame; a constant return is
// materialized immediately after the incoming r3 has been preserved.
// builds: GC/1.1
// flags: -Cpp_exceptions off -inline deferred
int variadic_true(const char* format, const char* file, int line, ...)
{
    return 1;
}
