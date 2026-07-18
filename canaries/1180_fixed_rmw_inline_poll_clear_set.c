// Read-direction sibling of 1179: only the direction field update changes,
// from clearing bit 15 to setting it. The inlined poll/clear tail and all six
// parameter-fed field schedules remain identical.
typedef volatile unsigned short vu16;
vu16 registers[32] : 0xCC005000;

static void wait_for_dma(void)
{
    while (registers[5] & 0x0200) {
    }
}

void clear_dma_interrupt(void)
{
    unsigned short temporary;
    temporary = registers[5];
    temporary = (unsigned short)((temporary & ~0x88) | 0x20);
    registers[5] = temporary;
}

static void read_dma(unsigned main_address, unsigned aram_address, unsigned length)
{
    registers[16] = (unsigned short)((registers[16] & ~0x03ff) | (unsigned short)(main_address >> 16));
    registers[17] = (unsigned short)((registers[17] & ~0xffe0) | (unsigned short)(main_address & 0xffff));
    registers[18] = (unsigned short)((registers[18] & ~0x03ff) | (unsigned short)(aram_address >> 16));
    registers[19] = (unsigned short)((registers[19] & ~0xffe0) | (unsigned short)(aram_address & 0xffff));
    registers[20] = (unsigned short)(registers[20] | 0x8000);
    registers[20] = (unsigned short)((registers[20] & ~0x03ff) | (unsigned short)(length >> 16));
    registers[21] = (unsigned short)((registers[21] & ~0xffe0) | (unsigned short)(length & 0xffff));
    wait_for_dma();
    clear_dma_interrupt();
}
