// Candidate execution probe; fresh reference-compiler objects pending.
// flags: -O0 -Cpp_exceptions off -pragma "cats off"
extern unsigned changing;
extern void first(unsigned);
extern void second(unsigned);
inline void visit(unsigned value) { first(value); second(value); }
void drive(unsigned value) { visit(value); value += 1; visit(value); second(value); }
void globals(void) { visit(changing); second(7); }
