// flags: -sdata 0 -sdata2 0
// Absolute global as an arithmetic operand: a separate base GPR (avoiding the
// sibling x) holds the address; @l folds into the load (lis r4; lwz r0,g@l(r4)).
extern int g;
int absreadx(int x){ return g + x; }
