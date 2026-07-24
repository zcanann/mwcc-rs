// builds: GC/1.2.5 GC/1.3 GC/1.3.2 GC/2.0 GC/2.0p1 GC/2.6 GC/2.7

typedef struct Node Node;
typedef struct Queue Queue;

struct Queue {
    Node* head;
};

struct Node {
    Queue* queue;
};

void set_head_to_owner(Node* node)
{
    node->queue->head = node;
}
