// flags: -sym on
/* Named assembly arguments retain typedef widths and pointed-to qualifiers. */
typedef unsigned long Count;
asm void scalar_parameters(register Count count, const register int* source,
                           register int* destination) {
    nofralloc
    lwz r6, 0(source)
    add r6, r6, count
    stw r6, 0(destination)
    blr
}
