// builds: 1.3 1.3.2 2.0 2.0p1 2.6 2.7

typedef struct Pair {
    int first;
    int second;
} Pair;

void assign_constant(Pair* pair)
{
    pair->first = pair->second = 16;
}

void assign_parameter(Pair* pair, int value)
{
    pair->first = pair->second = value;
}
