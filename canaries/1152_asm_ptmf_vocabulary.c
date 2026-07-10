// The ptmf (pointer-to-member-function) asm vocabulary — animal_crossing Runtime ptmf.c, flipped
// whole-file BYTE by this:
// - `Tag.field(rN)` Dot-form operands: a struct-TAG-qualified field offset as a displacement
//   (`lwz r5, PTMF.this_delta(r3)` -> `lwz r5, 0(r3)`), including a field that is a NAMED WORD-SIZED
//   UNION member (`union { void* f_addr; long ve_offset; } f_data;` lays out as a 4-byte field at its
//   aligned offset — previously "a named union member is not supported yet").
// - `bnelr- crN`: a prediction hint on a branch-to-link is DROPPED by mwcc (measured: `bnelr-` =
//   plain 4c 82 00 20), and the crN field selects BI (cr6 -> 26, cr7 -> 30).
// - `lwzx rD,rA,rB` (indexed load, the vtable dispatch) and `bctr` (count-register tail dispatch,
//   which also TERMINATES the function — no implicit trailing blr).
// - A VARIADIC asm signature `(...)` (no named parameters).
// (fire 641 — the third real-file flip)
typedef struct PTMF {
    long this_delta;
    long v_offset;
    union {
        void* f_addr;
        long ve_offset;
    } f_data;
} PTMF;

long __my_ptmf_test(PTMF* ptmf);
void __my_ptmf_scall(...);

asm long __my_ptmf_test(register PTMF* ptmf)
{
    nofralloc
    lwz       r5, PTMF.this_delta(r3)
    lwz       r6, PTMF.v_offset(r3)
    lwz       r7, PTMF.f_data(r3)
    li        r3, 0x1
    cmpwi     r5, 0
    cmpwi     cr6, r6, 0
    cmpwi     cr7, r7, 0
    bnelr-
    bnelr-    cr6
    bnelr-    cr7
    li        r3, 0
    blr
}

asm void __my_ptmf_scall(...)
{
    nofralloc
    lwz     r0, PTMF.this_delta(r12)
    lwz     r11, PTMF.v_offset(r12)
    lwz     r12, PTMF.f_data(r12)
    add     r3, r3, r0
    cmpwi   r11, 0
    blt-    cr0, loc_0x20
    lwzx    r12, r3, r12
    lwzx    r12, r12, r11
loc_0x20:
    mtctr   r12
    bctr
}
