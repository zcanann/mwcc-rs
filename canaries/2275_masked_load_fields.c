// Disjoint masked memory fields; the O0 compound-load owner still defers.
typedef unsigned int u32;
typedef unsigned short u16;
u32 masked(const u16* p) { return (p[0] & 15u) | ((u32)(p[1] & 15u) << 4); }
u32 top_bit(const u32* p) { return (p[0] & 1u) | (p[1] << 31); }
