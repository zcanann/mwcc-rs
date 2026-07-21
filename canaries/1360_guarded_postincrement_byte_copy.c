char* copy_guarded(char* destination, char* source, unsigned count)
{
    char* cursor;

    if (!destination)
        return destination;
    if (!count)
        return destination;
    cursor = destination;
    do {
        *cursor++ = *source++;
    } while (--count);
    return destination;
}
