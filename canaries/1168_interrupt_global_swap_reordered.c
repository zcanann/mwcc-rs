// AI declares the interrupt-state local before the callback local; allocation
// is driven by value lifetimes, not source declaration order.
typedef void (*Callback)(void);

static Callback callback_global;
extern int disable_interrupts(void);
extern void restore_interrupts(int enabled);

Callback register_callback(Callback replacement)
{
    int enabled;
    Callback old;
    old = callback_global;
    enabled = disable_interrupts();
    callback_global = replacement;
    restore_interrupts(enabled);
    return old;
}
