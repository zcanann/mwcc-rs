// The SDK callback-registrar shape: the incoming callback and the previous
// global callback both cross the disable/restore calls. Virtual live ranges
// assign old -> r31 and replacement -> r30; the disable result remains in r3
// as the restore argument.
typedef void (*Callback)(void);

static Callback callback_global;
extern int disable_interrupts(void);
extern void restore_interrupts(int enabled);

Callback register_callback(Callback replacement)
{
    Callback old;
    int enabled;
    old = callback_global;
    enabled = disable_interrupts();
    callback_global = replacement;
    restore_interrupts(enabled);
    return old;
}
