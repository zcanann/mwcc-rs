// flags: -O4,s

unsigned int xnor_right(unsigned int data, unsigned int count)
{
    unsigned int i;
    for (i = 0; i < count; ++i) {
        data = (data >> 1) |
               ((~(data ^ (data >> 7) ^ (data >> 15) ^ (data >> 23))) << 30) &
                   0x40000000;
    }
    return data;
}

unsigned int xnor_left(unsigned int data, unsigned int count)
{
    unsigned int i;
    for (i = 0; i < count; ++i) {
        data = (data << 1) |
               ((~(data ^ (data << 7) ^ (data << 15) ^ (data << 23))) >> 30) &
                   0x00000002;
    }
    return data;
}
