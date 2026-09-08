// flags: -Cpp_exceptions off -pragma "cats off"
typedef struct Node { unsigned tag; struct Node *next; unsigned value; } Node;
Node *head;
Node *pop(void) {
    Node *p;
    p = (Node *)(unsigned)&head[0];
    if (p) head = p->next;
    return p;
}
unsigned update(unsigned value) {
    Node *p;
    p = (Node *)(unsigned)&head[0];
    if (p) p->value = value;
    return p != 0;
}
