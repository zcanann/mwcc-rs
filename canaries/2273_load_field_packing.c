// Byte/halfword field insertion derived from Dolphin MD5 decoding.
typedef unsigned int u32; typedef unsigned char u8; typedef unsigned short u16;
u32 le2(const u8*p) { return p[0]|((u32)p[1]<<8); }
u32 be2(const u8*p) { return ((u32)p[0]<<8)|p[1]; }
u32 le3(const u8*p) { return p[0]|((u32)p[1]<<8)|((u32)p[2]<<16); }
u32 be3(const u8*p) { return ((u32)p[0]<<16)|((u32)p[1]<<8)|p[2]; }
u32 le4(const u8*p) { return p[0]|((u32)p[1]<<8)|((u32)p[2]<<16)|((u32)p[3]<<24); }
u32 be4(const u8*p) { return ((u32)p[0]<<24)|((u32)p[1]<<16)|((u32)p[2]<<8)|p[3]; }
u32 le4r(const u8*p) { return ((u32)p[3]<<24)|(((u32)p[2]<<16)|(((u32)p[1]<<8)|p[0])); }
u32 le4b(const u8*p) { return (p[0]|((u32)p[1]<<8))|(((u32)p[2]<<16)|((u32)p[3]<<24)); }
u32 half_le(const u16*p) { return p[0]|((u32)p[1]<<16); }
u32 half_be(const u16*p) { return ((u32)p[0]<<16)|p[1]; }
u32 separate(const u8*a,const u8*b) { return a[0]|((u32)b[0]<<8); }
u32 vol2(volatile u8*p) { return p[0]|((u32)p[1]<<8); }
u32 vol4(volatile u8*p) { return p[0]|((u32)p[1]<<8)|((u32)p[2]<<16)|((u32)p[3]<<24); }
void store4(u32*out,const u8*p) { *out=p[0]|((u32)p[1]<<8)|((u32)p[2]<<16)|((u32)p[3]<<24); }
