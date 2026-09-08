// flags: -Cpp_exceptions off -pragma "cats off"
typedef struct Node { unsigned tag; struct Node *next; unsigned value; } Node;
Node *nodes[16];
Node records[16];
unsigned *value_address(unsigned i) { return &nodes[i]->value; }
Node **next_address(unsigned i) { return &nodes[i]->next; }
unsigned *tag_address(void) { return &nodes[0]->tag; }
unsigned *record_address(unsigned i) { return &records[i].value; }
void write_value(unsigned i, unsigned value) { nodes[i]->value = value; }
void replace_next(unsigned i, Node *p) { nodes[i]->next = p; }
void replace_entry(unsigned i, Node *p) { nodes[i] = p; nodes[i]->next = 0; }
