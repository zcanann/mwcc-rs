// builds: GC/1.3.2 GC/2.6
// flags: -O0,p -Cpp_exceptions off -sdata 0 -sdata2 0 -pool off

void store_byte_offset(unsigned char* values, int index, unsigned char value)
{
    values[index + 1] = value;
}

void store_halfword_offset(unsigned short* values, int index, unsigned short value)
{
    values[index + 1] = value;
}

void store_word_offset(unsigned int* values, int index, unsigned int value)
{
    values[index + 1] = value;
}
