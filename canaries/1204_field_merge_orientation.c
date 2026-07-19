/* Characterize the base/insert orientation of disjoint integer field merges. */
unsigned mask_mask(unsigned a, unsigned b) {
    return (a & 0xffff0000) | (b & 0xffff);
}
unsigned mask_shift(unsigned a, unsigned b) {
    return (a & 0xffff0000) | (b >> 16);
}
unsigned partial_mask_shift(unsigned a, unsigned b) {
    return (a & 0xff000000) | (b >> 24);
}
