// Parameters cross the entering call in r31/r30, then feed a scheduled run of
// read-modify-writes to one fixed-address hardware register bank. The entering
// call's state stays in r3 for the final call while the RMW DAG uses r4-r7/r0.
typedef volatile unsigned short vu16;
vu16 registers[32] : 0xCC005000;

extern int enter_critical(void);
extern void leave_critical(int state);

void program_dma(unsigned address, unsigned length)
{
    int state;
    state = enter_critical();
    registers[24] = (unsigned short)((registers[24] & ~0x3ff) | (address >> 16));
    registers[25] = (unsigned short)((registers[25] & ~0xffe0) | (0xffff & address));
    registers[27] = (unsigned short)((registers[27] & ~0x7fff) | ((length >> 5) & 0xffff));
    leave_critical(state);
}
