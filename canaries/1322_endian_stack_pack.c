// builds: GC/1.1p1

extern int big_endian;
extern int append_bytes(void* buffer, const void* data, unsigned int size);

int pack16(void* buffer, const unsigned short data)
{
    unsigned char* selected;
    unsigned char* bytes;
    unsigned char swapped[sizeof(data)];
    if (big_endian) {
        selected = (unsigned char*)&data;
    } else {
        bytes = (unsigned char*)&data;
        selected = swapped;
        selected[0] = bytes[1];
        selected[1] = bytes[0];
    }
    return append_bytes(buffer, (const void*)selected, sizeof(data));
}

int pack32(void* buffer, const unsigned int data)
{
    unsigned char* selected;
    unsigned char* bytes;
    unsigned char swapped[sizeof(data)];
    if (big_endian) {
        selected = (unsigned char*)&data;
    } else {
        bytes = (unsigned char*)&data;
        selected = swapped;
        selected[0] = bytes[3];
        selected[1] = bytes[2];
        selected[2] = bytes[1];
        selected[3] = bytes[0];
    }
    return append_bytes(buffer, (const void*)selected, sizeof(data));
}

int pack64(void* buffer, const unsigned long long data)
{
    unsigned char* selected;
    unsigned char* bytes;
    unsigned char swapped[sizeof(data)];
    if (big_endian) {
        selected = (unsigned char*)&data;
    } else {
        bytes = (unsigned char*)&data;
        selected = swapped;
        selected[0] = bytes[7];
        selected[1] = bytes[6];
        selected[2] = bytes[5];
        selected[3] = bytes[4];
        selected[4] = bytes[3];
        selected[5] = bytes[2];
        selected[6] = bytes[1];
        selected[7] = bytes[0];
    }
    return append_bytes(buffer, (const void*)selected, sizeof(data));
}
