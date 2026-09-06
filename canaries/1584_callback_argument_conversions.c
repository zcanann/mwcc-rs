// flags: -Cpp_exceptions off -pragma "cats off"
// Registration scheduling must preserve the declared integer conversion.
typedef void (*Callback)(int, void*);
void handler(int, void*);
void byte_install(signed char, Callback);
void unsigned_byte_install(unsigned char, Callback);
void short_install(short, Callback);
void unsigned_short_install(unsigned short, Callback);
void word_install(int, Callback);
void finish(void);
extern int registration_id;

void signed_byte(void) { byte_install(255, handler); finish(); }
void unsigned_byte(void) { unsigned_byte_install(-1, handler); finish(); }
void signed_short(void) { short_install(0x10003, handler); finish(); }
void unsigned_short(void) { unsigned_short_install(-1, handler); finish(); }
void large_word(void) { word_install(0x12345678, handler); finish(); }
void forwarded_word(int id) { word_install(id, handler); finish(); }
void loaded_word(void) { word_install(registration_id, handler); finish(); }
