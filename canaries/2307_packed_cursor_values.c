struct Reader { unsigned char *cursor; unsigned count; };
int read_low(struct Reader *reader) {
    int value;
    value = (int)(*reader->cursor << 28) >> 28;
    reader->cursor++;
    return value;
}
int read_bits(struct Reader *reader) {
    int value;
    if (reader->count & 1) {
        value = (int)((*reader->cursor & 15) << 28) >> 28;
        reader->cursor++;
    } else value = (int)((*reader->cursor & 240) << 24) >> 28;
    reader->count++;
    return value;
}
struct OffsetReader { unsigned tag; unsigned count; unsigned char *cursor; };
int read_signed_five(struct OffsetReader *reader) {
    int value;
    value = (int)(*reader->cursor << 27) >> 27;
    reader->cursor += 3;
    reader->count++;
    return value;
}
int read_volatile(struct Reader volatile *reader) {
    int value;
    value = (int)(*reader->cursor << 28) >> 28;
    reader->cursor++;
    return value;
}
