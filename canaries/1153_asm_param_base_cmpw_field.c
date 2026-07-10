// The last two ptmf.c-family asm forms (these flipped the wind_waker/strikers/BfBB variants,
// completing all 7 corpus copies of ptmf.c):
// - A NAMED REGISTER PARAMETER as the memory-operand BASE: `lwz r5, PTMF.this_delta(ptmf)` — the
//   base resolves through the parameter's positional register (ptmf -> r3), in both the Tag.field
//   form and the plain displacement form (`lwz r6, 4(ptmf)`).
// - `cmpw crN, rA, rB` — the signed REGISTER compare into a non-cr0 field (CompareWordField,
//   opcode 31/XO 0: crf<<23 | a<<16 | b<<11), alongside the existing cmpwi crN immediate form.
// (fire 642)
typedef struct PTMF2 {
    long this_delta;
    long v_offset;
    union {
        void* f_addr;
        long ve_offset;
    } f_data;
} PTMF2;

asm long __my_ptmf_test2(register PTMF2* ptmf)
{
    nofralloc
    lwz       r5, PTMF2.this_delta(ptmf)
    lwz       r6, 4(ptmf)
    lwz       r7, PTMF2.f_data(ptmf)
    li        r3, 0x1
    cmpw      r5, r6
    cmpw      cr6, r6, r7
    cmpwi     cr7, r7, 0
    bnelr-
    bnelr-    cr6
    bnelr-    cr7
    li        r3, 0
    blr
}
