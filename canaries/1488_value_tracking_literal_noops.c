// Literal expression statements left by disabled assertion macros do not break
// a straight-line tracked local. The local remains substitutable into the
// return, allowing the pointer adjustment and member offset to fold into one
// load: `lwz r3,-24(r3); addi r3,r3,-32; blr`.
struct ValueTrackingNoopCell {
    struct ValueTrackingNoopCell* previous;
    struct ValueTrackingNoopCell* next;
    unsigned size;
};

unsigned value_tracking_literal_noops(void* pointer)
{
    struct ValueTrackingNoopCell* cell;

    (void)0;
    (void)0;
    cell = (void*)((unsigned)pointer - 32);
    (void)0;
    return (unsigned)cell->size - 32;
}
