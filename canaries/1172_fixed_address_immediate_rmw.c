// Single-node fixed-address read/modify/write schedules. The hardware-register
// base lives in r3 while the loaded and updated halfword flows through r0.
typedef volatile unsigned short vu16;
vu16 registers[32] : 0xCC005000;

void start_dma(void)
{
    registers[27] |= 0x8000;
}

void stop_dma(void)
{
    registers[27] &= ~0x8000;
}
