// flags: -Cpp_exceptions off -pragma "cats off"
typedef unsigned long long Tick;
volatile Tick previous;
Tick stable;
extern Tick clock(void);
extern void consume(unsigned value);
unsigned elapsed(void) { return (unsigned)(clock() - previous); }
void dispatch(void) { consume((unsigned)(clock() - previous) / 4); }
unsigned unqualified(void) { return (unsigned)(clock() - stable); }
