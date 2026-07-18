// The pointer parameter first crosses enter_critical in r31, then dies at the
// load. The masked result destructively reuses the same r31 home across the
// leave call: one saved register for two sequential source-level values.
extern int enter_critical(void);
extern void leave_critical(int state);

unsigned pointer_status(volatile unsigned short *registers)
{
    int state;
    unsigned value;
    state = enter_critical();
    value = registers[5] & 0x200;
    leave_critical(state);
    return value;
}
