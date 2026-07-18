// Implicit assignment conversion into byte and halfword objects. Older mwcc builds retain
// some signed narrowing operations even though the following stb/sth writes only the low bits;
// keep source width/signedness and addressing form separate so the build profile can describe
// that policy without keying codegen on a particular sample.

void char_from_int(char *p, int x)                   { *p = x; }
void char_from_uint(char *p, unsigned x)             { *p = x; }
void char_from_char(char *p, char x)                 { *p = x; }
void char_from_uchar(char *p, unsigned char x)       { *p = x; }

void uchar_from_int(unsigned char *p, int x)         { *p = x; }
void uchar_from_uint(unsigned char *p, unsigned x)   { *p = x; }
void uchar_from_char(unsigned char *p, char x)       { *p = x; }
void uchar_from_uchar(unsigned char *p, unsigned char x) { *p = x; }

void short_from_int(short *p, int x)                 { *p = x; }
void short_from_uint(short *p, unsigned x)           { *p = x; }
void short_from_short(short *p, short x)             { *p = x; }
void short_from_ushort(short *p, unsigned short x)   { *p = x; }

void ushort_from_int(unsigned short *p, int x)       { *p = x; }
void ushort_from_uint(unsigned short *p, unsigned x) { *p = x; }
void ushort_from_short(unsigned short *p, short x)   { *p = x; }
void ushort_from_ushort(unsigned short *p, unsigned short x) { *p = x; }

void indexed_char_from_int(char *p, int i, int x)    { p[i] = x; }
void displaced_short_from_int(short *p, int x)       { p[3] = x; }

extern int wide_global;
int wide_call(void);
void char_from_add(char *p, int a, int b)             { *p = a + b; }
void char_from_shift(char *p, int x)                  { *p = x >> 3; }
void char_from_negate(char *p, int x)                 { *p = -x; }
void char_from_divide(char *p, int x, int y)          { *p = x / y; }
void char_from_global(char *p)                        { *p = wide_global; }
void char_from_call(char *p)                          { *p = wide_call(); }
void char_from_literal(char *p)                       { *p = 257; }
void short_from_literal(short *p)                     { *p = 65537; }
