// C++ creates tentative and explicit-zero local symbols at declaration.
// flags: -Cpp_exceptions off -pragma "cats off"
static int first;
static char buffer[9];
static short third[2];
int exported;
int initialized = 7;
static int explicit_zero = 0;
