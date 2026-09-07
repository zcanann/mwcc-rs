// flags: -Cpp_exceptions off -pragma "cats off"
// ABI-backed casts must participate in loop call liveness before emission.
unsigned float_sum(float x, unsigned n) { unsigned sum = 0; while (n) { sum = sum + (unsigned)x; x = x + 0.5f; n--; } return sum; }
unsigned local_sum(float x, unsigned n) { float y = x; unsigned sum = 0; while (n) { sum = sum + (unsigned)y; y = y + 1.0f; n--; } return sum; }
unsigned double_sum(double x, unsigned n) { unsigned sum = 0; while (n) { sum = sum + (unsigned)x; x = x + 1.25; n--; } return sum; }
unsigned nested_cast(float x, unsigned n) { unsigned sum = 0; while (n) { sum = sum + (unsigned)(int)(unsigned)x; x = x + 0.5f; n--; } return sum; }
unsigned array_sum(const float *p, unsigned n) { unsigned sum = 0; while (n) { sum = sum + (unsigned)*p; p++; n--; } return sum; }
struct Sample { double value; };
unsigned member_sum(const struct Sample *p, unsigned n) { unsigned sum = 0; while (n) { sum = sum + (unsigned)p->value; n--; } return sum; }
