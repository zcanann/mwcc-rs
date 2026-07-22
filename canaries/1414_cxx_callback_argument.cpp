// builds: GC/1.2.5n GC/1.3.2 GC/2.6
// flags: -O4,s -inline off -Cpp_exceptions off -pragma "cats off" -func_align 32

typedef void (*Callback)(void*);

extern void install_callback(void* first, void* second, Callback callback);

static void callback(void* context)
{
}

static void pass_callback(void* first, void* second)
{
    install_callback(first, second, callback);
}
