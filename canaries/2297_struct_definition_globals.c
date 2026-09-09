struct Node { unsigned value; struct Node *next; };
static struct Queue { struct Node *head; struct Node *tail; } queue, other;
struct Local { unsigned pad; unsigned head; };
struct Node *read_head(void) { return queue.head; }
struct Node *read_tail(void) { return queue.tail; }
struct Node **head_address(void) { return &queue.head; }
void clear_queue(void) { queue.head = queue.tail = 0; }
void set_head(struct Node *node) { queue.head = node; }
void move_queue(void) { other.head = queue.head; other.tail = queue.tail; }
unsigned shadow_queue(struct Local *queue) { return queue->head; }
struct Node *read_after_shadow(void) { return queue.head; }
