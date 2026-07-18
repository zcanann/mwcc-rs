typedef volatile unsigned short vu16;
vu16 registers[32] : 0xCC005000;

void clear_low_bit(void)
{
    registers[0] &= ~0x0001;
}

void clear_middle_bit(void)
{
    registers[1] &= ~0x0020;
}

void clear_high_bit(void)
{
    registers[2] &= ~0x8000;
}

void set_mixed_bits(void)
{
    registers[3] |= 0x8021;
}
