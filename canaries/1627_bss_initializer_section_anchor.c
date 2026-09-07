// A section-relative initializer must include the second object's BSS offset.
// flags: -Cpp_exceptions off -pragma "cats off"
static char first[9];
static char second[13];
char* pointers[3] = {first, second, first};
