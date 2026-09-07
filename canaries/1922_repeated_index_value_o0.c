// flags: -Cpp_exceptions off -pragma "cats off"
// Retained frontier: repeated load plus its original index in the return.
extern unsigned words[16];
unsigned repeated(unsigned i) { unsigned a = words[i]; return a + words[i] + i; }
