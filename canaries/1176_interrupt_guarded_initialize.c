// A guarded, interrupt-protected initialization transaction. Both parameters
// and the interrupt state survive calls; scalar globals, a callback, and one
// fixed-address hardware register are initialized before a final global reload.
typedef volatile unsigned short vu16;
typedef void (*Callback)(void);

vu16 registers[32] : 0xCC005000;

extern int init_flag;
extern Callback callback;
extern unsigned stack_pointer;
extern unsigned free_blocks;
extern unsigned *block_lengths;

extern int disable_interrupts(void);
extern void restore_interrupts(int state);
extern void set_interrupt_handler(int interrupt, Callback handler);
extern void unmask_interrupts(unsigned mask);
extern void check_size(void);
extern void ar_handler(void);

unsigned initialize(unsigned *stack_index_addr, unsigned num_entries)
{
    int old;
    unsigned short refresh;

    if (init_flag == 1) {
        return 0x4000;
    }

    old = disable_interrupts();
    callback = (Callback)0;
    set_interrupt_handler(6, ar_handler);
    unmask_interrupts(0x02000000);
    stack_pointer = 0x4000;
    free_blocks = num_entries;
    block_lengths = stack_index_addr;
    refresh = (unsigned short)(registers[13] & 0xff);
    registers[13] = (unsigned short)((registers[13] & ~0xff) | (refresh & 0xff));
    check_size();
    init_flag = 1;
    restore_interrupts(old);
    return stack_pointer;
}
