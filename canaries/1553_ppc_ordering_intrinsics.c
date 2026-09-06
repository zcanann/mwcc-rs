/* Ordering intrinsics are target instructions and observable memory barriers. */
void __sync(void);
void __isync(void);
void __eieio(void);
void ordered_stores(int* output, int value) {
    output[0] = value;
    __sync();
    output[1] = value;
    __eieio();
    output[2] = value;
    __isync();
}
void standalone_sync(void) { __sync(); }
