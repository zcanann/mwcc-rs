// (x >> n) & low-mask fuses into one rlwinm: rotate-left (32-n), keep the masked
// low bits. The classic (value >> 16) & 0x7FFF shape (e.g. an LCG's output).
typedef unsigned u32;
u32 hibits(u32 x){ return (x >> 16) & 0x7FFF; }
