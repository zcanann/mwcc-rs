// builds: GC/1.1p1

extern int big_endian;
extern int read_bytes(void* buffer, void* data, unsigned int size);

int unpack16(void* buffer, unsigned short* data)
{
    int error; unsigned char* selected; unsigned char* bytes; unsigned char swapped[sizeof(data)];
    if (big_endian) selected = (unsigned char*)data; else selected = swapped;
    error = read_bytes(buffer, selected, 2);
    if (!big_endian && error == 0) { bytes = (unsigned char*)data; bytes[0] = selected[1]; bytes[1] = selected[0]; }
    return error;
}
int unpack32(void* buffer, unsigned int* data)
{
    int error; unsigned char* selected; unsigned char* bytes; unsigned char swapped[sizeof(data)];
    if (big_endian) selected = (unsigned char*)data; else selected = swapped;
    error = read_bytes(buffer, selected, 4);
    if (!big_endian && error == 0) { bytes = (unsigned char*)data; bytes[0] = selected[3]; bytes[1] = selected[2]; bytes[2] = selected[1]; bytes[3] = selected[0]; }
    return error;
}
int unpack64(void* buffer, unsigned long long* data)
{
    int error; unsigned char* selected; unsigned char* bytes; unsigned char swapped[sizeof(data)];
    if (big_endian) selected = (unsigned char*)data; else selected = swapped;
    error = read_bytes(buffer, selected, 8);
    if (!big_endian && error == 0) { bytes = (unsigned char*)data; bytes[0] = selected[7]; bytes[1] = selected[6]; bytes[2] = selected[5]; bytes[3] = selected[4]; bytes[4] = selected[3]; bytes[5] = selected[2]; bytes[6] = selected[1]; bytes[7] = selected[0]; }
    return error;
}
