// The inverse critical-section allocator transaction: decrement a global
// pointer, optionally report its pointed-to length, update cursor/free globals,
// restore interrupts, then reload the cursor as the return value.
extern int enter_critical(void);
extern void leave_critical(int state);

extern unsigned stack_pointer;
extern unsigned *block_lengths;
extern unsigned free_blocks;

unsigned free_block(unsigned *out_length)
{
    int state;
    state = enter_critical();
    block_lengths--;
    if (out_length) {
        *out_length = *block_lengths;
    }
    stack_pointer -= *block_lengths;
    free_blocks++;
    leave_critical(state);
    return stack_pointer;
}
