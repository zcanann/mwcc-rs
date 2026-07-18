// Promote a queued request when idle, service either its full remaining length
// or one chunk, then advance the three mutable request fields.
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
extern unsigned chunk_size;
extern Callback active_callback;
extern void start_dma(unsigned type, unsigned source, unsigned dest, unsigned length);

void service_queue(void)
{
    if ((pending == (Request *)0) && queue_head) {
        pending = queue_head;
        queue_head = queue_head->next;
    }
    if (pending) {
        if (pending->length <= chunk_size) {
            if (pending->type == 0)
                start_dma(pending->type, pending->source, pending->dest, pending->length);
            else
                start_dma(pending->type, pending->dest, pending->source, pending->length);
            active_callback = pending->callback;
        } else {
            if (pending->type == 0)
                start_dma(pending->type, pending->source, pending->dest, chunk_size);
            else
                start_dma(pending->type, pending->dest, pending->source, chunk_size);
        }
        pending->length -= chunk_size;
        pending->source += chunk_size;
        pending->dest += chunk_size;
    }
}
