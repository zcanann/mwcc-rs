// flags: -Cpp_exceptions off -pragma "cats off"
int input;
unsigned int unsigned_input;
volatile int observed;
void signed_3(int *out) { int q; q = input / 3; *out = input + q; }
void unsigned_3(unsigned int *out) { unsigned int q; q = unsigned_input / 3; *out = unsigned_input + q; }
void signed_7(int *out) { int q; q = input / 7; *out = input + q; }
void unsigned_7(unsigned int *out) { unsigned int q; q = unsigned_input / 7; *out = unsigned_input + q; }
void signed_160(int *out) { int q; q = input / 160; *out = input + q; }
void unsigned_160(unsigned int *out) { unsigned int q; q = unsigned_input / 160; *out = unsigned_input + q; }
void volatile_input(int *out) { int q; q = observed / 160; *out = observed + q; }
void joined(int *out, int flag) { int q; if (flag) *out = 0; q = input / 160; if (q) *out = input + q; else *out = 0; }
