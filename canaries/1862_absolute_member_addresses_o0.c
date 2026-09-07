// Absolute member addresses and dependent pointer arguments.
// flags: -Cpp_exceptions off -pragma "cats off"
struct Bank { unsigned head; float value; unsigned words[4]; struct { unsigned tag; float value; } nested; };
volatile struct Bank PORT : 0xCC008000;
volatile float* value_address(void) { return &PORT.value; }
volatile unsigned* word_address(void) { return &PORT.words[2]; }
volatile float* nested_address(void) { return &PORT.nested.value; }
void save_address(volatile float** out) { *out = &PORT.value; }
float* cast_address(void) { return &((struct Bank*)0x12347ffc)->value; }
volatile unsigned* variable_word(unsigned i) { return &PORT.words[i]; }
