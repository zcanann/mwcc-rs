// flags: -sym on
/* The source qualifier belongs to the aggregate behind the argument pointer. */
typedef struct Vector { float x; float y; float z; } Vector;
asm void aggregate_parameters(const register Vector* source, register Vector* destination) {
    nofralloc
    lfs f0, Vector.x(source)
    stfs f0, Vector.x(destination)
    blr
}
