// De Morgan: when both operands of a bitwise and/or are complemented leaves, mwcc
// folds to a single instruction — `~a & ~b` is `nor(a,b)`, `~a | ~b` is `nand(a,b)`
// — rather than complementing each operand and then combining (not; not; and/or).
int  dm_nor(int a, int b)            { return ~a & ~b; }
int  dm_nand(int a, int b)           { return ~a | ~b; }
unsigned dm_nor_u(unsigned a, unsigned b) { return ~a & ~b; }
