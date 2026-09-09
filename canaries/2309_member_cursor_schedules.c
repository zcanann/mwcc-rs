struct Reader { unsigned char *cursor; unsigned count; unsigned char first, second, third; };
void one_field(struct Reader *r) { r->first = *r->cursor & 63; r->cursor += 3; }
void three_fields(struct Reader *r) {
    r->first = *r->cursor >> 2;
    r->second = *r->cursor & 15;
    r->third = *r->cursor >> 5;
    r->cursor += 2;
}
void initialize_three(struct Reader *r, unsigned char *input) {
    r->cursor = input; r->count = 7;
    r->first = *r->cursor >> 2;
    r->second = *r->cursor & 15;
    r->third = *r->cursor >> 5;
    r->cursor += 2;
}
void initialize_one(struct Reader *r, unsigned char *input) {
    r->cursor = input;
    r->first = *r->cursor & 63;
    r->cursor += 3;
}
unsigned counted_one(struct Reader *r) {
    if (!(r->count & 7)) { r->first = *r->cursor & 63; r->cursor += 3; r->count += 4; }
    return r->count;
}
void overlapping_final(struct Reader *r) {
    r->first = *r->cursor & 63;
    ((unsigned char *)&r->cursor)[3] = *r->cursor;
    r->cursor++;
}
