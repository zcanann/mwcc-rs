// Exported C++ definitions can also use a section-relative initializer.
// flags: -Cpp_exceptions off -pragma "cats off"
char first[9];
char second[13];
char* pointers[3] = {first, second, first};
