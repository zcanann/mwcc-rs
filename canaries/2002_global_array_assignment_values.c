// flags: -Cpp_exceptions off -pragma "cats off"
unsigned left[32], right[32];
signed char bytes[32];
short halves[32];
extern unsigned get_value(void);
void clear(unsigned i) { left[i] = right[i] = 0; }
void fill(unsigned i, unsigned value) { left[i] = right[i] = value; }
unsigned result(unsigned i, unsigned value) { return right[i] = value + 3; }
unsigned narrow(unsigned i, unsigned value) { return left[i] = bytes[i] = value; }
unsigned narrow_half(unsigned i, unsigned value) { return left[i] = halves[i] = value; }
void call_value(unsigned i) { left[i] = right[i] = get_value(); }
typedef struct Item { unsigned tag; unsigned value; } Item;
void member_value(unsigned i, Item *p) { left[i] = p->value; }
void computed_value(unsigned i, unsigned a, unsigned b) { left[i] = a * b + 7; }
