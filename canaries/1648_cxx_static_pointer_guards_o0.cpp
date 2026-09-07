// Older frontends guard pointer initialization even for const and null values.
// flags: -Cpp_exceptions off -pragma "cats off" -O0
char* short_text() { static char* p="abc"; return p; }
const char* long_text() { static const char* p="123456789"; return p; }
const char* fixed_text() { static const char* const p="long fixed text"; return p; }
int* null_pointer() { static int* p=0; return p; }
