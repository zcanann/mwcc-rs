// Two dereferences as a multiply operand: the left load coalesces onto its
// pointer register (which dies at the load), the right takes the scratch — the
// allocator supplying the temporary a deferral needed (Phase D).
int twoderefmul(int* p, int* q, int x){ return (*p + *q) * x; }
