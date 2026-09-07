// Exported BSS symbols appear between the surrounding function definitions.
// flags: -Cpp_exceptions off -pragma "cats off"
char first[9];
int before() { return 1; }
char second[13];
int after() { return 2; }
char tail[17];
char* pointer = second;
