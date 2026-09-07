// flags: -Cpp_exceptions off -pragma "cats off"
// A pair of member reads owns one pointer value within the expression.
struct Pair { unsigned short a, b; unsigned x, y; };
extern struct Pair *pair;
unsigned product(void) { return pair->a * pair->b; }
unsigned sum(void) { return pair->x + pair->y; }
unsigned difference(void) { return pair->x - pair->y; }
unsigned changed(struct Pair *next) { unsigned first = pair->x + pair->y; pair = next; return first + pair->x + pair->y; }
