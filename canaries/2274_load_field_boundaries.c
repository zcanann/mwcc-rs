// Controls for field overlap, sign extension, truncation, and narrow casts.
typedef unsigned int u32;
typedef unsigned char u8;
typedef unsigned short u16;
u32 overlap(const u8* p) { return p[0] | ((u32)p[1] << 4); }
u32 truncating(const u16* p) { return p[0] | ((u32)p[1] << 24); }
u32 signed_base(const short* p) { return (u32)p[0] | ((u32)p[1] << 16); }
u32 signed_insert(const u8* a, const short* b) { return a[0] | ((u32)b[0] << 8); }
u32 narrowed(const u32* p) { return (u8)p[0] | ((u32)(u8)p[1] << 8); }
u32 separated(const u8* p) { return p[0] | ((u32)p[1] << 16); }
u32 zero_shift(const u8* p) { return p[0] | ((u32)p[1] << 0); }
u32 dynamic_shift(const u8* p, u32 shift) { return p[0] | ((u32)p[1] << shift); }
u32 retained_base(const u8* p, u32* out) { *out = p[0] | ((u32)p[1] << 8); return (u32)p; }
struct Fields { u8 lo; u8 hi; };
u32 member_pair(const struct Fields* p) { return p->lo | ((u32)p->hi << 8); }
u32 deref_pair(const u8* a, const u8* b) { return *a | ((u32)*b << 8); }
