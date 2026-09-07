// Function-scope pointer images retain their string relocations.
// flags: -Cpp_exceptions off -pragma "cats off"
char* first(void) { static char* text="abc"; return text; }
char* second(void) { static char* text="123456789"; return text; }
