// A wide call result retained across address construction and forwarded intact.
typedef struct Node { struct Node* next; struct Node* prev; } Node;
static Node queues[4];
extern unsigned long long begin_wide(void);
extern void end_wide(unsigned long long state);
void forward_wide(unsigned priority, Node* block) {
    unsigned long long state=begin_wide();
    Node* queue=&queues[priority];
    block->next=queue;
    queue->prev=block;
    end_wide(state);
}
