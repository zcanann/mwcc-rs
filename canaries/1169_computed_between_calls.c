// A computed fixed-address memory value is produced between two calls. The
// entering call's state remains in r3 for the leaving call; the mask result is
// assigned to r31 and survives that call.
extern int enter_critical(void);
extern void leave_critical(int state);

unsigned fixed_status(void)
{
    int state;
    unsigned value;
    state = enter_critical();
    value = ((volatile unsigned short *)0xCC005000)[5] & 0x200;
    leave_critical(state);
    return value;
}
