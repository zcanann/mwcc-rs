// Static and exported C++ BSS share physical declaration order.
// flags: -Cpp_exceptions off -pragma "cats off"
char global_first[9];
static char local_first[13];
char global_second[17];
static char local_second[21];
char* pointers[4] = {local_second, global_second, local_first, global_first};
