// Unsigned division by a power of two is a logical right shift, which fuses with
// a following mask into one rlwinm — the canonical LCG `rand` tail. The value is
// a just-stored global still live in its register, reused (not reloaded). A
// SIGNED division must NOT fuse (it rounds toward zero, not a floor).
unsigned udivmask_seed;
unsigned udivmask(void) {
    return (udivmask_seed = udivmask_seed * 1103515245 + 12345) / 65536 & 0x7fff;
}
int udivmask_signed(int x) { return (x / 65536) & 0x7fff; }
