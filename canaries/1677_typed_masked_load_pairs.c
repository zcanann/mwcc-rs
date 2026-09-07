// Independent indices, narrow elements, discontiguous masks, and volatile reads.
// Parameter names deliberately shadow file-scope arrays of a different type.
// flags: -Cpp_exceptions off -pragma "cats off"
extern unsigned left[128], right[128];
unsigned bytes(unsigned i, unsigned j, const unsigned char* left, const unsigned char* right) {
    return left[i & 15] ^ right[j & 3];
}
int halves(unsigned i, unsigned j, const short* left, const short* right) {
    return left[i & 3] - right[j & 15];
}
unsigned wide(unsigned i, unsigned j, const unsigned* left, const unsigned* right) {
    return left[i & 85] + right[j & 15];
}
unsigned reverse(unsigned i, unsigned j, const unsigned* left, const unsigned* right) {
    return right[j & 3] - left[i & 85];
}
unsigned observed(unsigned i, unsigned j, const volatile unsigned* left, const volatile unsigned* right) {
    return left[i & 3] + right[j & 15];
}
