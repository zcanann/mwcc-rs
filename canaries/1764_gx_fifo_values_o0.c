// GX FIFO and rotate-insert lowering, with execution coverage.
// flags: -O0 -Cpp_exceptions off -pragma "cats off"
typedef union { unsigned char byte; unsigned word; float real; double wide; } Fifo;
#define FIFO (*(volatile Fifo*)0xCC008000)
extern unsigned fetch(void);
void literal(void) { FIFO.byte = 97; FIFO.word = 42; }
void computed(unsigned a, unsigned b) { FIFO.word = a + b; }
void called(void) { FIFO.word = fetch(); }
void float_literal(void) { FIFO.real = 1.5f; }
void float_sum(float a, float b) { FIFO.real = a + b; }
void double_literal(void) { FIFO.wide = 2.5; }
