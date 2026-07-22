// builds: GC/1.2.5n
// flags: -Cpp_exceptions off

extern void observe(int);

void one_call_helper(int value)
{
    if (value > 0) {
        observe(1);
        return;
    }
    if (value < 0) {
        observe(-1);
    }
}

void auto_inline_void_return(int value)
{
    observe(0);
    one_call_helper(value);
    observe(2);
}
