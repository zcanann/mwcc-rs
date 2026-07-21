// builds: GC/2.0p1
// flags: -Cpp_exceptions off -char unsigned -inline off

char* byte_class_tokenizer(char* string, const char* control, char** next_token)
{
    unsigned char* cursor;
    unsigned char* control_cursor;
    unsigned char map[32];
    int unused_count;

    for (int index = 0; index < 32; index++)
    {
        map[index] = 0;
    }

    control_cursor = (unsigned char*)control;
    do
    {
        map[*control_cursor >> 3] |= 1 << (*control_cursor & 7);
    } while (*control_cursor++ != '\0');

    cursor = string ? (unsigned char*)string : (unsigned char*)*next_token;
    while (map[(*cursor >> 3) & 31] & (1 << (*cursor & 7)) && *cursor != '\0')
    {
        cursor++;
    }

    string = (char*)cursor;
    while (*cursor != '\0')
    {
        if (map[(*cursor >> 3) & 31] & (1 << (*cursor & 7)))
        {
            *cursor = '\0';
            cursor++;
            break;
        }
        cursor++;
    }

    *next_token = (char*)cursor;
    if (string == (char*)cursor)
    {
        string = 0;
    }
    return string;
}
