// A fixed-register status read whose mask already narrows the result to u16.
// mwcc writes the rlwinm directly into the ABI result register, with no second
// return-type truncation.
typedef volatile unsigned short vu16;
vu16 registers[32] : 0xCC005000;

unsigned short interrupt_status(void)
{
    return (unsigned short)(registers[5] & 0x0200);
}
