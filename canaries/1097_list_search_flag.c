struct Node { int value; struct Node *next; };
int contains(struct Node *p, int key) {
    while (p) { if (p->value == key) return 1; p = p->next; }
    return 0;
}
