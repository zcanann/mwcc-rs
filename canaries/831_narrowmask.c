// A narrow (char/short) value masked with a constant whose run sits entirely within the
// value's own bit-width needs NO promotion: mwcc masks the raw register (`char a & 0xf` is
// `clrlwi r3,r3,28`), because the char sign/zero-extension only touches bits the mask clears.
// Previously we emitted a redundant `extsb r0,r3; clrlwi r3,r0,28`.
//
// The mask run must start within the narrow value's low `width` bits (big-endian bit
// 32-width onward): a mask that reaches the extension bits (`a & 0x1ff` on a char) keeps the
// promotion via the normal path, and a bitwise OR (which needs the high bits) always keeps it.
int char_mask_low(char a)            { return a & 0xf; }      // clrlwi r3,r3,28, no extsb
int char_mask_byte(char a)           { return a & 0xff; }     // clrlwi r3,r3,24
int uchar_mask_low(unsigned char a)  { return a & 0xf; }
int short_mask_low(short a)          { return a & 0xf; }
int short_mask_12(short a)           { return a & 0xfff; }
int char_mask_mid(char a)            { return a & 0x30; }     // mid run, still within the byte
int char_mask_operand(char a)        { return (a & 0xf) + 1; }
int char_mask_exceeds(char a)        { return a & 0x1ff; }    // reaches bit 8 -> keeps the extsb
int char_or(char a)                  { return a | 0xf; }      // OR keeps the extsb
