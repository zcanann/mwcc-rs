// Constant-store scheduling across target and control-flow shapes. The values are distinct so
// each function exposes whether the compiler preloads every value, serializes through r0, or
// splits a guarded preload run around its first store. Repeated constants are covered separately
// because both compiler lines intentionally reuse one scratch materialization.

int g0, g1, g2, g3, g4, g5, g6;

void straight_globals(void)
{
    g0 = 1;
    g1 = 2;
    g2 = 3;
}

void straight_pointer(int *p)
{
    p[0] = 1;
    p[1] = 2;
}

void guarded_two(int condition)
{
    if (condition) {
        g0 = 1;
        g1 = 2;
    }
}

void guarded_five(int condition)
{
    if (condition) {
        g0 = 1;
        g1 = 2;
        g2 = 3;
        g3 = 4;
        g4 = 5;
    }
}

void straight_six(void)
{
    g0 = 1;
    g1 = 2;
    g2 = 3;
    g3 = 4;
    g4 = 5;
    g5 = 6;
}

void guarded_seven(int condition)
{
    if (condition) {
        g0 = 1;
        g1 = 2;
        g2 = 3;
        g3 = 4;
        g4 = 5;
        g5 = 6;
        g6 = 7;
    }
}

void guarded_pointer(int condition, int *p)
{
    if (condition) {
        p[0] = 1;
        p[1] = 2;
    }
}

void guarded_repeated(int condition)
{
    if (condition) {
        g0 = 7;
        g1 = 7;
        g2 = 7;
    }
}
