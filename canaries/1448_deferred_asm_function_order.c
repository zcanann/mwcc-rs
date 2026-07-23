// builds: GC/2.7 GC/3.0a3p1 Wii/1.0
// flags: -inline deferred
// Deferred emission changes at 4.x: GC 2.7 assembles asm immediately and then
// reverses compiled bodies; GC 3/Wii reverse the complete source stream.
void compiled_first(void) { }

asm void asm_middle(void) {
    nofralloc
    li r3, 7
    blr
}

void compiled_last(void) { }
