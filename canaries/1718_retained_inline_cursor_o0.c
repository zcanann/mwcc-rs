// Candidate execution probe; fresh reference-compiler objects pending.
// flags: -O0 -Cpp_exceptions off -pragma "cats off"
struct Node { struct Node* next; unsigned value; };
extern void first(struct Node*);
extern void second(struct Node*);
inline void visit(struct Node* node) { first(node); second(node); }
void walk(struct Node* cursor) {
    for (; cursor; cursor = cursor->next) { visit(cursor); cursor->value = 0; }
}
