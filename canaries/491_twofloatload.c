// Two float loads as a multiply operand: the left load goes to a fresh virtual
// the allocator places on a free FPR (f2, avoiding z in f1), the right to the
// scratch f0 — a deferral the register allocator (Phase D) now removes.
float twofloatload(float* p, float* q, float z){ return (*p + *q) * z; }
