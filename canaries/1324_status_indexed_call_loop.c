// builds: GC/1.1p1

extern int append_word(void* buffer, unsigned int value);

int append_words(void* buffer, const unsigned int* data, int count)
{
    int error;
    int i;
    for (i = 0, error = 0; error == 0 && i < count; i++) {
        error = append_word(buffer, data[i]);
    }
    return error;
}
