// A scalar global read in BOTH an if-condition AND the guarded call — mwcc loads it ONCE into the
// argument register r3, tests it there, and REUSES it for the call (no reload). This is the Runtime
// __fini_cpp_exceptions shape (`if (fragmentID != -2) { __unregister_fragment(fragmentID);
// fragmentID = -2; }`): a non-leaf LR-only frame, the global loaded once, an equality guard, a direct
// external call reusing the value, and a constant store back to the same global:
//   stwu r1,-16; mflr r0; stw r0,20(r1); lwz r3,g; cmpwi r3,C; b<!REL> skip; bl ext; li r0,C2;
//   stw r0,g; skip: lwz r0,20(r1); mtlr r0; addi r1,r1,16; blr
// Previously deferred ("a global read in both an if-condition and its body needs value reuse across the
// branch"). Advances the real Runtime/__init_cpp_exceptions.c to its next construct. (fire 636)
extern void __unregister_fragment(int);
static int fragmentID = -2;
void ggr_fini(void)
{
    if (fragmentID != -2) {
        __unregister_fragment(fragmentID);
        fragmentID = -2;
    }
}
