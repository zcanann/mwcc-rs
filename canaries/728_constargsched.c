// A non-leaf call's READY argument materializations (li/lis of constants) are
// scheduled into the mflr->LR-save latency gap: up to two before the `stw
// r0,20(r1)`, the rest after. (A frame-relative `&local` addi stays after.)
extern void cas1(int);
extern void cas2(int, int);
extern void cas3(int, int, int);
void cas_one(void)   { cas1(7); }
void cas_two(void)   { cas2(7, 9); }
void cas_three(void) { cas3(1, 2, 3); }
