// A narrow-typed function returning `var OP constant` of add/sub/or/xor/mul reads
// the operand raw and truncates the result once (mwcc `addi r0,r3,5; clrlwi r3,r0,24`
// for unsigned, `extsb`/`extsh` for signed), rather than extending the operand first
// (redundant under the trailing truncation). BitAnd and shift-left fold the constant
// into the truncating rlwinm and are left to the full optimization; two-variable
// forms and div/mod/shift-right still defer.
unsigned char add5(unsigned char a)  { return a + 5; }
unsigned char or80(unsigned char a)  { return a | 0x80; }
unsigned char mul3(unsigned char a)  { return a * 3; }
char          inc(char a)            { return a + 1; }
short         add100(short a)         { return a + 100; }
