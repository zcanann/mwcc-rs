// Two globals as a multiply operand: the allocator places the first global's
// load in a free register (avoiding x), the second in the scratch — a case that
// deferred before the register allocator (Phase D) could supply the temporary.
extern int g, h;
int twoglobmul(int x){ return (g + h) * x; }
