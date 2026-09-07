// Captures the remaining allocated-frame owner limitation across calls.
// flags: -Cpp_exceptions off -pragma "cats off"
typedef union { unsigned char byte; unsigned word; } Fifo;
#define FIFO (*(volatile Fifo*)0xCC008000)
extern unsigned fetch(void), first(void), second(void);
extern unsigned __rlwimi(unsigned, unsigned, unsigned, unsigned, unsigned);
void reused(unsigned a, unsigned b) { FIFO.byte = 97; FIFO.word = a + b; FIFO.word = fetch(); }
unsigned called(void) { return __rlwimi(first(),second(),9,4,27); }
