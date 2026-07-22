unsigned int bit_reverse(unsigned int data)
{
    unsigned int work;
    unsigned int index;
    unsigned int low_count = 0;
    unsigned int high_shift = 1;

    work = 0;
    for (index = 0; index < 32; ++index) {
        if (index > 15) {
            if (index == 31) {
                work |= ((data & (1U << 31)) >> 31) & 1U;
            } else {
                work |= (data & (1U << index)) >> high_shift;
                high_shift += 2;
            }
        } else {
            work |= (data & (1U << index)) << (31 - index - low_count);
            ++low_count;
        }
    }
    return work;
}
