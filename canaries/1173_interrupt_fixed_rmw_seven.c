// Seven scheduled updates to one fixed-address hardware-register bank. Four
// incoming parameters cross the entering call, while its result stays in r3
// for the leaving call. This is the ARAM DMA programming shape.
typedef volatile unsigned short vu16;
vu16 registers[32] : 0xCC005000;

extern int enter_critical(void);
extern void leave_critical(int state);

void program_aram_dma(unsigned direction, unsigned main_address,
                      unsigned aram_address, unsigned length)
{
    int state;
    state = enter_critical();
    registers[16] = (unsigned short)(registers[16] & ~0x3ff) | (unsigned short)(main_address >> 16);
    registers[17] = (unsigned short)(registers[17] & ~0xffe0) | (unsigned short)(main_address & 0xffff);
    registers[18] = (unsigned short)(registers[18] & ~0x3ff) | (unsigned short)(aram_address >> 16);
    registers[19] = (unsigned short)(registers[19] & ~0xffe0) | (unsigned short)(aram_address & 0xffff);
    registers[20] = (unsigned short)((registers[20] & ~0x8000) | (direction << 15));
    registers[20] = (unsigned short)(registers[20] & ~0x3ff) | (unsigned short)(length >> 16);
    registers[21] = (unsigned short)(registers[21] & ~0xffe0) | (unsigned short)(length & 0xffff);
    leave_critical(state);
}
