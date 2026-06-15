// flags: -sdata 0 -sdata2 0
// A store materializes the base (lis) before the value, folding @l into stw.
int g;
void absstoreconst(void){ g = 5; }
