// A call input can remain in an ABI register without an explicit copy.
typedef struct Node { struct Node* next; struct Node* prev; } Node;
static Node queues[4];
extern unsigned begin_state(void);
extern void end_state(unsigned state);
extern void end_variadic(unsigned state, ...);
void clear_queues(void) { unsigned i; for (i=0;i<4;i++) { Node* queue=&queues[i]; queue->next=queue; queue->prev=queue; } }
unsigned push_queue(unsigned priority, Node* block) {
    unsigned state=begin_state();
    Node* queue=&queues[priority];
    queue->prev->next=block;
    block->prev=queue->prev;
    block->next=queue;
    queue->prev=block;
    end_state(state);
    return 1;
}
void forward_parameter(unsigned state, unsigned priority, Node* block) {
    Node* queue=&queues[priority];
    block->next=queue;
    queue->prev=block;
    end_state(state);
}
void forward_variadic(unsigned priority, Node* block) {
    unsigned state=begin_state();
    Node* queue=&queues[priority];
    block->next=queue;
    queue->prev=block;
    end_variadic(state, 31);
}
extern double begin_float(void);
extern void end_float(double state);
void forward_float(double* out, const double* input) {
    double state=begin_float();
    *out=input[0]+input[1];
    end_float(state);
}
