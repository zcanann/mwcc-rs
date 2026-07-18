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

extern Request *queue_hi;
extern Request *tail_hi;
extern Request *queue_lo;
extern Request *tail_lo;
extern Request *pending_hi;
extern Request *pending_lo;
extern Callback callback_hi;
extern Callback callback_lo;
extern unsigned chunk_size;
extern void start_dma(unsigned type, unsigned source, unsigned dest, unsigned length);
extern int disable_interrupts(void);
extern void restore_interrupts(int enabled);

void callback_fallback(unsigned unused) {}

void pop_task(void)
{
    if (queue_hi) {
        if (queue_hi->type == 0)
            start_dma(queue_hi->type, queue_hi->source, queue_hi->dest, queue_hi->length);
        else
            start_dma(queue_hi->type, queue_hi->dest, queue_hi->source, queue_hi->length);
        callback_hi = queue_hi->callback;
        pending_hi = queue_hi;
        queue_hi = queue_hi->next;
    }
}

void service_queue(void)
{
    if ((pending_lo == (Request *)0) && queue_lo) {
        pending_lo = queue_lo;
        queue_lo = queue_lo->next;
    }
    if (pending_lo) {
        if (pending_lo->length <= chunk_size) {
            if (pending_lo->type == 0)
                start_dma(pending_lo->type, pending_lo->source, pending_lo->dest, pending_lo->length);
            else
                start_dma(pending_lo->type, pending_lo->dest, pending_lo->source, pending_lo->length);
            callback_lo = pending_lo->callback;
        } else {
            if (pending_lo->type == 0)
                start_dma(pending_lo->type, pending_lo->source, pending_lo->dest, chunk_size);
            else
                start_dma(pending_lo->type, pending_lo->dest, pending_lo->source, chunk_size);
        }
        pending_lo->length -= chunk_size;
        pending_lo->source += chunk_size;
        pending_lo->dest += chunk_size;
    }
}

void post_request(Request *request, unsigned owner, unsigned type, unsigned priority,
                  unsigned source, unsigned dest, unsigned length, Callback callback)
{
    int enabled;

    request->next = (Request *)0;
    request->owner = owner;
    request->type = type;
    request->source = source;
    request->dest = dest;
    request->length = length;

    if (callback)
        request->callback = callback;
    else
        request->callback = (Callback)&callback_fallback;

    enabled = disable_interrupts();
    switch (priority) {
    case 0:
        if (queue_lo)
            tail_lo->next = request;
        else
            queue_lo = request;
        tail_lo = request;
        break;
    case 1:
        if (queue_hi)
            tail_hi->next = request;
        else
            queue_hi = request;
        tail_hi = request;
        break;
    }

    if ((pending_hi == (Request *)0) && (pending_lo == (Request *)0)) {
        pop_task();
        if (pending_hi == (Request *)0)
            service_queue();
    }
    restore_interrupts(enabled);
}
