// Pointer-array indices scale by the source element size.
// flags: -Cpp_exceptions off -pragma "cats off"
char bytes[16];
short halves[16];
int words[16];
double doubles[16];
char* byte_pointer = &bytes[3];
short* half_pointer = &halves[3];
int* word_pointer = &words[3];
double* double_pointer = &doubles[3];
