int maskfold(int x){ return (x >> 2) & (4 + 4); }
int bittest(int x){ return (x & (1 << 3)) == 0; }
int fieldfold(int x){ return (x & (0xf << 4)) >> 4; }
