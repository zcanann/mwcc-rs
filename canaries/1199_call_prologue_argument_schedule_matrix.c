// Argument preparation in the non-leaf prologue latency slot. ALU computations, register copies,
// and an immediate are independent of the link-register save and may fill its gap; a memory load
// remains after the completed frame setup. The two-argument case establishes the measured slot cap.

int sink1(int);
int sink2(int, int);

int computed_argument(int a, int b) { return sink1(a + b); }
int repeated_argument(int a)        { return sink2(a, a); }
int immediate_argument(void)        { return sink1(0); }
int loaded_argument(int *p)         { return sink1(*p); }
int two_computed_arguments(int a, int b, int c, int d)
{
    return sink2(a + b, c + d);
}
