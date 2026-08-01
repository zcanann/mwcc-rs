// builds: GC/1.1 GC/2.6
// flags: -O4,p -inline auto -sdata 0 -sdata2 0

typedef struct Dividend {
    int first;
    int second;
} Dividend;

typedef struct Divisor {
    int value;
} Divisor;

int computed_member_modulo(Dividend* dividend, Divisor* divisor)
{
    return (dividend->first + dividend->second) % divisor->value;
}
