// builds: 1.3 1.3.2 2.0 2.0p1 2.6 2.7

typedef struct Thread {
    char context[712];
} Thread;

void store_two_system_pointers(Thread* thread)
{
    *(void**)0x800000D8 = &thread->context;
    *(void**)0x800000E4 = thread;
}
