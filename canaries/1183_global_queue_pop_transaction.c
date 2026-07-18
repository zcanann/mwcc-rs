// A queue head is reused through a conditional call setup, then reloaded after
// the call before callback/pending/head globals are committed.
typedef struct Request Request;
typedef void (*Callback)(unsigned);

struct Request {
    Request *next;
    unsigned owner;
    unsigned type;
    unsigned priority;
    unsigned source;
    unsigned dest;
    unsigned length;
    Callback callback;
};

extern Request *queue_head;
extern Request *pending;
extern Callback active_callback;
extern void start_dma(unsigned type, unsigned source, unsigned dest, unsigned length);

void pop_queue(void)
{
    if (queue_head) {
        if (queue_head->type == 0) {
            start_dma(queue_head->type, queue_head->source, queue_head->dest, queue_head->length);
        } else {
            start_dma(queue_head->type, queue_head->dest, queue_head->source, queue_head->length);
        }
        active_callback = queue_head->callback;
        pending = queue_head;
        queue_head = queue_head->next;
    }
}
