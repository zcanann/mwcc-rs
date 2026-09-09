struct Reader { unsigned char *cursor; unsigned count; unsigned char first, second; };
void initialize(struct Reader *reader, unsigned char *input) {
    reader->cursor = input;
    reader->count = 2;
    reader->first = (*reader->cursor & 112) >> 4;
    reader->second = *reader->cursor & 15;
    reader->cursor++;
}
void refresh(struct Reader *reader) {
    reader->first = (*reader->cursor & 112) >> 4;
    reader->second = *reader->cursor & 15;
    reader->cursor++;
}
unsigned guarded_refresh(struct Reader *reader) {
    if (!(reader->count & 15)) {
        reader->first = (*reader->cursor & 112) >> 4;
        reader->second = *reader->cursor & 15;
        reader->cursor++;
        reader->count += 2;
    }
    return reader->count;
}
void volatile_refresh(struct Reader volatile *reader) {
    reader->first = (*reader->cursor & 112) >> 4;
    reader->second = *reader->cursor & 15;
    reader->cursor++;
}
struct Other { unsigned tag; unsigned short left, right; unsigned char *cursor; };
void other_fields(struct Other *reader) {
    reader->left = *reader->cursor & 63;
    reader->right = *reader->cursor >> 3;
    reader->cursor += 2;
}
struct VolatileMember { unsigned char * volatile cursor; unsigned count; unsigned char first, second; };
void volatile_member(struct VolatileMember *reader) {
    reader->first = (*reader->cursor & 112) >> 4;
    reader->second = *reader->cursor & 15;
    reader->cursor++;
}
void unknown_store(struct Reader *reader, unsigned char **alias, unsigned char *replacement) {
    reader->first = *reader->cursor;
    *alias = replacement;
    reader->second = *reader->cursor;
    reader->cursor++;
}
union Overlap { unsigned char *cursor; unsigned char bytes[4]; };
void overlap_store(union Overlap *reader) {
    reader->bytes[3] = *reader->cursor & 128;
    reader->bytes[2] = *reader->cursor;
}
