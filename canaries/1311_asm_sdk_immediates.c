// Dolphin SDK spellings shared by __ppc_eabi_init.cpp across several projects.
void asm_sdk_target(void);

asm void asm_sdk_flush(register void* address, register unsigned int size)
{
    nofralloc
    lis r5, ~0
    ori r5, r5, ~14
    and r5, r5, r3
    subic. r4, r4, 8
    blr
}

asm void asm_sdk_frame_directive(void)
{
    fralloc
    bl asm_sdk_target
}
