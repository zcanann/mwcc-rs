// flags: -sym on -Cpp_exceptions off
/* An assembly body omits the C entry scope in the shared debug ordinal stream. */
void vector_begin(void);
static asm void vector_stub(void) {
entry vector_begin
    nofralloc
    li r3, 1
    blr
}
int ordinary_follower(void) {
    return 1;
}
