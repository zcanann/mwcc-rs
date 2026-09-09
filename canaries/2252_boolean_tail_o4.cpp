// flags: 
typedef unsigned char u8; typedef signed char s8;
typedef unsigned short u16; typedef signed short s16;
u8 byte_equal(int a) { return a == 1; }
s8 signed_not_equal(int a) { return a != 1; }
u16 half_less(int a) { return a < 0; }
s16 signed_less(int a) { return a < 3; }
u8 byte_greater(unsigned a, unsigned b) { return a > b; }
s16 signed_equal(int a, int b) { return a == b; }
u8 byte_not(int a) { return !a; }
s16 signed_not(int a) { return !a; }
bool bool_equal(int a) { return a == 1; }
bool bool_less(int a) { return a < 3; }
bool bool_not(int a) { return !a; }
u8 cast_equal(int a) { return (u8)(a == 1); }
s16 cast_not(int a) { return (s16)!a; }
u8 arithmetic_byte(u8 a) { return a + 5; }
s16 arithmetic_half(s16 a) { return a + 100; }
unsigned word_equal(int a) { return a == 1; }
extern "C" u8 unmangled_equal(int a) { return a == 1; }
extern "C" s16 unmangled_not(int a) { return !a; }
