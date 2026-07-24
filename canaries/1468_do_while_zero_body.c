// builds: GC/1.2.5 GC/1.3 GC/1.3.2 GC/2.0 GC/2.0p1 GC/2.6 GC/2.7

void once(int* output, int value)
{
    do {
        output[0] = value;
        output[1] = 0;
    } while (0);
}
