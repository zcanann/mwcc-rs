// Large-constant multiply of a *loaded* global: mwcc builds the constant high in
// one register and loads the global into another (lis t; lwz g; addi r0,t,lo;
// mullw d,g,r0). The high-temp and load go to fresh virtuals so the allocator
// keeps them distinct — the inline version collided when the dest was the scratch.
int g;
void scaleg(void){ g = 0x41C64E6D * g; }
