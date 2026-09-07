// flags: -Cpp_exceptions off -pragma "cats off"
// Legacy address schedules must not overwrite a still-live subscript.
extern unsigned words[16];
extern unsigned char bytes[256];
#define PORT (*(volatile unsigned *)0xcc008000)
void guarded(unsigned i, unsigned flag) { unsigned a = words[i]; if (flag) { PORT = a; PORT = i; } PORT = words[i]; }
void stored(unsigned i, unsigned value) { words[i] = value; if (value) { PORT = words[i]; PORT = i; } }
void constant_store(unsigned i, unsigned flag) { words[i] = 7; if (flag) { PORT = words[i]; PORT = i; } }
void byte_index(unsigned i, unsigned flag) { unsigned a = bytes[(unsigned char)i]; if (flag) { PORT = a; PORT = i; } PORT = bytes[(unsigned char)i]; }
