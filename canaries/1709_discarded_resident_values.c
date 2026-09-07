// Candidate execution probe; fresh compiler-reference objects pending.
// flags: -Cpp_exceptions off -pragma "cats off"
unsigned discard_value(unsigned value) { value; return value + 7; }
unsigned* discard_pointer(unsigned* pointer) { pointer; return pointer + 2; }
