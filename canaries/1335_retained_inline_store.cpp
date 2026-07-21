// flags: -inline noauto -O4,s

typedef unsigned char u8;

extern u8 output[];

inline static void write_byte(u8 data)
{
    output[0] = data;
}

void write_wrapper(u8 data)
{
    write_byte(data);
}
