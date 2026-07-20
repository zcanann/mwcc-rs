/* Cross-statement store/guard scheduling from Dolphin's OSClearContext. */
typedef struct Context {
    unsigned char padding[416];
    unsigned short mode;
    unsigned short state;
} Context;

Context* current : 0x800000D8;

void clear_context(register Context* context) {
    context->mode = 0;
    context->state = 0;
    if (context == current) {
        current = 0;
    }
}
