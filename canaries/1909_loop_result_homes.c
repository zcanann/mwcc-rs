// flags: -Cpp_exceptions off -pragma "cats off"
// Loop conditions retain one value home through entry and back edges.
extern unsigned bump(unsigned);
extern unsigned down(unsigned);
unsigned until(unsigned value, unsigned limit) { unsigned next; while (value < limit) { next = bump(value); value = next; } return value; }
unsigned two_loops(unsigned value, unsigned limit) { unsigned next; while (value > limit) { next = down(value); value = next; } while (value < limit) { next = bump(value); value = next; } return value; }
unsigned counted(unsigned value, unsigned n) { unsigned next; while (n) { next = bump(value); value = next; n--; } return value; }
