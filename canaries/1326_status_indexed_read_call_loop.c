// builds: GC/1.1p1
// flags: -pragma "cats off" -Cpp_exceptions off -fp_contract off -sdata 0 -sdata2 0 -pool off -inline on,noauto -common off

extern int read_byte(void* buffer, unsigned char* value);
extern int read_half(void* buffer, unsigned short* value);
extern int read_word(void* buffer, unsigned int* value);
extern int read_doubleword(void* buffer, unsigned long long* value);

int read_bytes(void* buffer, unsigned char* data, int count)
{
    int error;
    int i;
    for (i = 0, error = 0; error == 0 && i < count; i++) {
        error = read_byte(buffer, &(data[i]));
    }
    return error;
}

int read_halves(void* buffer, unsigned short* data, int count)
{
    int error;
    int i;
    for (i = 0, error = 0; error == 0 && i < count; i++) {
        error = read_half(buffer, &(data[i]));
    }
    return error;
}

int read_words(void* buffer, unsigned int* data, int count)
{
    int error;
    int i;
    for (i = 0, error = 0; error == 0 && i < count; i++) {
        error = read_word(buffer, &(data[i]));
    }
    return error;
}

int read_doublewords(void* buffer, unsigned long long* data, int count)
{
    int error;
    int i;
    for (i = 0, error = 0; error == 0 && i < count; i++) {
        error = read_doubleword(buffer, &(data[i]));
    }
    return error;
}
