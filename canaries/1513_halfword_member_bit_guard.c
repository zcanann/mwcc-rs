// A halfword member bit-test exposes whether the mask itself records CR0 or
// whether the build retains a separate unsigned comparison against zero.
// builds: GC/1.2.5 GC/1.2.5n
// flags: -Cpp_exceptions off

typedef struct Context {
    unsigned char prefix[418];
    unsigned short state;
    unsigned int flags;
} Context;

extern void active(void);

asm void preceding_asm(void)
{
    nofralloc
    blr
}

void halfword_member_bit_guard(Context* context)
{
    if (context->state & 1) {
        active();
    }
}

void halfword_member_bit_clear(Context* context)
{
    if (!(context->state & 1)) {
        active();
    }
}

void word_member_bit_guard(Context* context)
{
    if (context->flags & 1) {
        active();
    }
}

void parameter_bit_guard(unsigned int flags)
{
    if (flags & 1) {
        active();
    }
}
