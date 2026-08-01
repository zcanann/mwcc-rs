// builds: GC/1.3.2
// flags: -O0,p -Cpp_exceptions off -sdata 0 -sdata2 0 -pool off

extern short create_handle(void);
extern void consume_handle(int);

void promoted_narrow_call_result_local(void)
{
    int handle;

    handle = create_handle();
    consume_handle(handle);
    consume_handle(handle);
}
