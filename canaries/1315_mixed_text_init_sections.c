// builds: GC/1.2.5n
// flags: -pragma "cats off" -Cpp_exceptions off

extern void init_target(void);
extern void text_target(void);

__declspec(section ".init") asm void init_entry(void) {
    nofralloc
    bl init_target
    blr
}

void text_entry(void) {
    text_target();
}
