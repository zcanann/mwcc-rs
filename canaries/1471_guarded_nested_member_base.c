// builds: GC/1.2.5 GC/1.3 GC/1.3.2 GC/2.0 GC/2.0p1 GC/2.6 GC/2.7

typedef struct Node Node;
typedef struct Queue Queue;

struct Queue {
    Node* head;
    Node* tail;
};

struct Node {
    Queue* queue;
    Node* next;
    Node* previous;
};

void link_tail(Node* node)
{
    Node* previous;

    previous = node->queue->tail;
    if (previous == 0) {
        node->queue->head = node;
    } else {
        previous->next = node;
    }
    node->previous = previous;
    node->next = 0;
    node->queue->tail = node;
}
