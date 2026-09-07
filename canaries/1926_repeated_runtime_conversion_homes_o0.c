// flags: -Cpp_exceptions off -pragma "cats off"
// Retained frontier: parameter promotion across repeated runtime calls.
unsigned two_floats(float x, float y, unsigned n) { unsigned r = x; unsigned s = y; return r + s + n; }
unsigned reused(float x, unsigned n) { unsigned r = x; return r + (unsigned)x + n; }
