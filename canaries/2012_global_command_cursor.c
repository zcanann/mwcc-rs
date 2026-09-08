// flags: -Cpp_exceptions off -pragma "cats off"
unsigned short *cursor;
unsigned *words;
struct Record { unsigned a, b, c; };
struct Record *records;
void emit(unsigned value) { *cursor = value; cursor++; }
void retreat(void) { --cursor; }
void advance(unsigned count) { cursor += count; }
void advance_words(void) { words++; }
void advance_records(void) { records++; }
unsigned count;
void advance_loaded(void) { cursor += count; }
