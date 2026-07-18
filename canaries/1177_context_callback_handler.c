// A large context-frame interrupt handler: program one fixed-address status
// register, install a temporary context around an optional global callback,
// then restore the caller's context. The callback pointer is loaded once for
// both the condition and indirect call.
typedef volatile unsigned short vu16;
typedef void (*Callback)(void);

typedef struct Context {
    double alignment;
    unsigned char payload[704];
} Context;
typedef void (*InterruptHandler)(short interrupt, Context *context);

vu16 registers[32] : 0xCC005000;
extern Callback callback;
extern void clear_context(Context *context);
extern void set_current_context(Context *context);
extern void install_interrupt_handler(InterruptHandler handler);

static void interrupt_handler(short interrupt, Context *context);

void install_test_handler(void)
{
    install_interrupt_handler(interrupt_handler);
}

static void interrupt_handler(short interrupt, Context *context)
{
    Context exception_context;
    unsigned short temporary;

    temporary = registers[5];
    temporary = (unsigned short)((temporary & ~0x88) | 0x20);
    registers[5] = temporary;

    clear_context(&exception_context);
    set_current_context(&exception_context);

    if (callback) {
        (*callback)();
    }

    clear_context(&exception_context);
    set_current_context(context);
}
