// flags: -Cpp_exceptions off -pragma "cats off"
struct Table { unsigned guard; unsigned words[8]; unsigned tail; };
extern struct Table *table;
void bits(unsigned i, unsigned a, unsigned b) { table->words[i] = (table->words[i] & ~(1u << 18)) | ((a & 1) << 18); table->words[i] = (table->words[i] & ~(1u << 19)) | ((b & 1) << 19); *(volatile unsigned *)0xcc008000 = table->words[i]; }
unsigned index_tail(struct Table *p, unsigned i, unsigned v) { p->words[i] = p->words[i] + v; return i; }
void constant(struct Table *p, unsigned i) { p->words[i] = 0x12345678; }
