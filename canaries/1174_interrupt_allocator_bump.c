// An allocator transaction protected by enter/leave calls. The requested
// length and old cursor occupy r30/r31 across the leaving call; the intervening
// global and pointed-to updates are latency-scheduled through r4/r5/r0.
extern int enter_critical(void);
extern void leave_critical(int state);

extern unsigned stack_pointer;
extern unsigned *block_lengths;
extern unsigned free_blocks;

unsigned allocate_block(unsigned length)
{
    unsigned result;
    int state;

    state = enter_critical();
    result = stack_pointer;
    stack_pointer += length;
    *block_lengths = length;
    block_lengths++;
    free_blocks--;
    leave_critical(state);
    return result;
}
