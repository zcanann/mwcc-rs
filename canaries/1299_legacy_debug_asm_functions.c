// Monolithic legacy DWARF composes aggregate data, typed asm parameters, and
// one line-table row per emitted asm instruction. `nofralloc` and labels do not
// consume text addresses or line records.
// builds: GC/1.3.2
// flags: -sdata 0 -sdata2 0 -O4,p -inline auto -sym on

typedef struct Dispatch {
    long this_delta;
    long table_offset;
    union {
        void* address;
        long entry_offset;
    } target;
} Dispatch;

const Dispatch dispatch_null = {0, 0, 0};

asm void dispatch_test(register Dispatch* dispatch) {
nofralloc
lwz r5, Dispatch.this_delta(dispatch)
lwz r6, Dispatch.table_offset(dispatch)
lwz r7, Dispatch.target(dispatch)
li r3, 1
cmpwi cr0, r5, 0
bnelr cr0
li r3, 0
blr
}

asm void dispatch_call(...) {
nofralloc
lwz r0, Dispatch.this_delta(r12)
lwz r11, Dispatch.table_offset(r12)
lwz r12, Dispatch.target(r12)
add r3, r3, r0
cmpwi r11, 0
blt @1
lwzx r12, r3, r12
lwzx r12, r12, r11
@1
mtctr r12
bctr
}
