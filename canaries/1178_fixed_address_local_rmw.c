// A one-node fixed-address RMW expressed through a narrow local temporary.
// This is semantically the same leaf DAG as a direct compound assignment, but
// mwcc gives the explicit load/update/store form its own register schedule.
typedef volatile unsigned short vu16;
vu16 registers[32] : 0xCC005000;

void clear_interrupt(void)
{
    unsigned short temporary;
    temporary = registers[5];
    temporary = (unsigned short)((temporary & ~0x88) | 0x20);
    registers[5] = temporary;
}
