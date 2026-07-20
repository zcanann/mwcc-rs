// The old Runtime constructor runner spells the NULL-terminated function-pointer walk as a `for`
// loop whose initializer and step live in the header. With peepholes disabled, GC/1.2.5n retains
// two otherwise redundant label edges, uses its linkage-first frame, and dispatches through LR:
// `mflr; stw; stwu; ...; b; b; b condition; mtlr r12; blrl; ...`. This is semantically the same
// walker as the newer `while (*p) { (**p)(); p++; }` form and belongs to the shared loop lowering.
// builds: GC/1.2.5n
typedef void (*VoidFunc)(void);
extern VoidFunc _ctors[];

#pragma peephole off
void legacy_for_pointer_walker(void) {
    VoidFunc* constructor;
    for (constructor = _ctors; *constructor; constructor++) {
        (*constructor)();
    }
}
#pragma peephole reset
