// builds: GC/2.0p1
// flags: -Cpp_exceptions off -char unsigned -inline off

char* ascii_uppercase(char* input)
{
    char* cursor = input;
    while (*cursor != '\0')
    {
        *cursor = (*cursor >= 'a' && *cursor <= 'z' ? *cursor - 32 : *cursor);
        cursor++;
    }
    return input;
}
