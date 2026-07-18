signed char signed_byte;
unsigned char unsigned_byte;
signed short signed_halfword;
unsigned short unsigned_halfword;

void shift_signed_byte(void)
{
    signed_byte >>= 1;
}

void shift_unsigned_byte(void)
{
    unsigned_byte >>= 1;
}

void shift_signed_halfword(void)
{
    signed_halfword >>= 1;
}

void shift_unsigned_halfword(void)
{
    unsigned_halfword >>= 1;
}
