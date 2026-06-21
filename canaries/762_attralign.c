// A struct member can carry __attribute__((aligned(n))) (the post-macro form of
// ATTRIBUTE_ALIGN(n), pervasive in the game's headers) between its type and name.
// The attribute is skipped, honouring the requested alignment so every following
// member offset stays exact; multi-dimensional array members lay out as the
// product of their dimensions.
struct Mixed {
    short a;                                       /* 0x00 */
    unsigned char __attribute__((aligned(4))) buf[32];  /* 0x04 */
    unsigned char tail;                            /* 0x24 */
    unsigned char grid[3][16];                     /* 0x25 -> 0x55 */
    short trailer;                                 /* 0x56 */
};
int read_tail(struct Mixed *p) { return p->tail; }
int read_trailer(struct Mixed *p) { return p->trailer; }
