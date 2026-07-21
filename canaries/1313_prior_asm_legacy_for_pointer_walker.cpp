// A preceding asm definition suppresses GC/1.2.5n's file-level optimization
// of the old Runtime constructor-table for-loop. The loop retains redundant
// source edges and the canonical linkage-first prologue despite global O4.
// builds: GC/1.2.5n
extern "C" {
typedef void (*VoidFunc)(void);
extern VoidFunc _ctors[];

asm void preceding_asm(void)
{
    nofralloc
    blr
}

void prior_asm_legacy_for_pointer_walker(void)
{
    VoidFunc* constructor;
    for (constructor = _ctors; *constructor; constructor++) {
        (*constructor)();
    }
}
}
