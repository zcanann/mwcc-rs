// Initializer references must bind to defined local zero-storage symbols.
// flags: -Cpp_exceptions off -pragma "cats off"
static int first;
static char buffer[9];
static short third[2];
int* first_ptr = &first;
char* buffer_ptr = buffer;
short* third_ptr = third;
