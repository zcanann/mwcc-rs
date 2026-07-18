typedef struct Request Request;
typedef void (*Callback)(unsigned);

extern Request *queue_hi;
extern Request *queue_lo;
extern Request *pending_hi;
extern Request *pending_lo;
extern Callback callback_hi;
extern Callback callback_lo;
extern unsigned chunk_size;
extern volatile int init_flag;
extern void register_callback(void (*callback)(void));

void interrupt_handler(void) {}

void initialize_queue(void)
{
    if (1 == init_flag)
        return;

    queue_hi = queue_lo = (Request *)0;
    chunk_size = 4096;
    register_callback(&interrupt_handler);
    pending_hi = (Request *)0;
    pending_lo = (Request *)0;
    callback_hi = (Callback)0;
    callback_lo = (Callback)0;
    init_flag = 1;
}
