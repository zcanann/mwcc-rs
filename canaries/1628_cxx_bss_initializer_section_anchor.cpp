// C++ retains declaration order and the older builds' BSS section anchor.
// flags: -Cpp_exceptions off -pragma "cats off"
static char first[9];
static char second[13];
char* pointers[3] = {first, second, first};
