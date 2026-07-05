struct Node { struct Node *next; };
void walk(struct Node *p) { while (p) p = p->next; }
