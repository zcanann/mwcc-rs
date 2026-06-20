// A global initializer element may be a constant *expression*, not just a
// literal: enum constants, parentheses, arithmetic, shifts, bitwise ops, and
// integer casts all fold to the stored value (the store then truncates to the
// element width). This mirrors decomp table macros like
// `DATA_MAKE_NUM(dir, file) = ((dir)+(file))`.
enum { DIR = 5 };
const int constexprinit[4] = {
    ((DIR) + (0x36)),
    (1 << 8) | 0x0F,
    (0xFF00 >> 4),
    (unsigned char)0x1FF,
};
