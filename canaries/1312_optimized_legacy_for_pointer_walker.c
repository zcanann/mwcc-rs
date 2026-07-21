// GC/1.2.5n's optimized Runtime constructor walker keeps the for-header source
// form but removes the redundant peephole-disabled label edges and interleaves
// table-address formation with its linkage-first prologue.
// builds: GC/1.2.5n
typedef void (*VoidFunc)(void);
extern VoidFunc _ctors[];

void optimized_legacy_for_pointer_walker(void)
{
    VoidFunc* constructor;
    for (constructor = _ctors; *constructor; constructor++) {
        (*constructor)();
    }
}
