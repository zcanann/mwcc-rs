// flags: -sym on
/* Equal shapes retain declaration identity; aliases reuse their original type. */
typedef float A[3][4];
typedef float B[4][4];
typedef float C[3][4];
typedef A D;
asm void first(const register A m) { nofralloc
blr
}
asm void second(const register B m) { nofralloc
blr
}
asm void third(const register C m) { nofralloc
blr
}
asm void fourth(const register D m) { nofralloc
blr
}
asm void fifth(const register A m) { nofralloc
blr
}
asm void sixth(const register float m[3][4]) { nofralloc
blr
}
