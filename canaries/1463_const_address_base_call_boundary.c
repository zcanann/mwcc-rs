// builds: 1.3 1.3.2 2.0 2.0p1 2.6 2.7

void sink(void);

void store_around_call(int* pointer)
{
    *(int**)0x800000D8 = pointer;
    sink();
    *(int**)0x800000E4 = pointer;
}
