// builds: 1.3 1.3.2 2.0 2.0p1 2.6 2.7

typedef struct Node {
    char prefix[712];
    unsigned short active;
    char gap[50];
    struct Node* next;
} Node;

typedef struct NodeQueue {
    Node* head;
    Node* tail;
} NodeQueue;

int contains(Node* thread)
{
    Node* cursor;
    if (thread->active == 0) {
        return 0;
    }
    for (cursor = ((NodeQueue*)0x800000DC)->head; cursor; cursor = cursor->next) {
        if (thread == cursor) {
            return 1;
        }
    }
    return 0;
}

void sink(void);

void ordinal_probe(void)
{
    sink();
}
