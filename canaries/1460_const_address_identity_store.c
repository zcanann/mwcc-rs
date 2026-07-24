// builds: 1.3 1.3.2 2.0 2.0p1 2.6 2.7

typedef struct Thread {
    char context[712];
} Thread;

void store_first_member_address(Thread* thread)
{
    *(void**)0x800000D8 = &thread->context;
}

void store_cancelled_dereference(Thread* thread)
{
    *(void**)0x800000D8 = &*thread;
}
