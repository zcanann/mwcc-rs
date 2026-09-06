// flags: -sym on
/* Row types preserve unsigned-long source identity and unqualified pointers. */
typedef unsigned long Count;
typedef Count Counts[2][3];
typedef Count (*CountRow)[3];
asm void array_argument(register Counts values, register Count* output) {
    nofralloc
    lwz r5, 0(values)
    stw r5, 0(output)
    blr
}
asm void row_pointer_argument(register CountRow values, register Count* output) {
    nofralloc
    lwz r5, 0(values)
    stw r5, 0(output)
    blr
}
