// Explicit load/shift/mask expressions are the control group for source-level bit-field reads.
// Build 163 extracts true bit-fields in the load's result register, but otherwise-identical
// explicit expressions still route the load through r0. Keep byte, halfword, word, member,
// dereference, and index forms beside true bit-fields so provenance cannot be mistaken for width.

struct NarrowFields {
    unsigned char byte;
    unsigned short half;
    unsigned word;
};

unsigned member_byte_mask(struct NarrowFields *p)  { return p->byte & 0xf; }
unsigned member_byte_shift(struct NarrowFields *p) { return (p->byte >> 2) & 3; }
int member_byte_shift_int(struct NarrowFields *p)   { return (p->byte >> 2) & 3; }
unsigned member_half_mask(struct NarrowFields *p)  { return p->half & 0x7f; }
unsigned member_half_shift(struct NarrowFields *p) { return (p->half >> 3) & 0x1f; }
int member_half_shift_int(struct NarrowFields *p)   { return (p->half >> 3) & 0x1f; }
unsigned member_word_shift(struct NarrowFields *p) { return (p->word >> 3) & 0x1f; }

unsigned deref_byte_shift(unsigned char *p)   { return (*p >> 1) & 7; }
int deref_byte_shift_int(unsigned char *p)     { return (*p >> 1) & 7; }
unsigned deref_half_shift(unsigned short *p)  { return (*p >> 4) & 0x3f; }
unsigned index_byte_shift(unsigned char *p, int i) { return (p[i] >> 2) & 3; }

struct PackedBytes {
    unsigned char byte_a : 2;
    unsigned char byte_b : 3;
};
struct PackedHalves {
    unsigned short half_a : 5;
    unsigned short half_b : 7;
};
struct PackedWords {
    unsigned word_a : 6;
    unsigned word_b : 17;
};

int bit_field_byte(struct PackedBytes *p) { return p->byte_b; }
int bit_field_half(struct PackedHalves *p) { return p->half_b; }
int bit_field_word(struct PackedWords *p) { return p->word_b; }
