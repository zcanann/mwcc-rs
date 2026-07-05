struct Node { int value; struct Node *next; };
struct Node *find(struct Node *p, int key) {
    while (p) { if (p->value == key) return p; p = p->next; }
    return 0;
}
