// Register divide with a complex dividend: the dividend computes into the scratch
// (its temporary a virtual the allocator places), then divw — place_operand_or_scratch
// reaching the division path. (complex)/(complex) stays guarded.
int divcomplex(int a, int b, int c, int d, int x){ return ((a + b) * (c + d)) / x; }
