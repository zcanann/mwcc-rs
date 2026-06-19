int rsh(int *p){ return (p[0] >> 2) & 3; }
int lsh(int *p){ return (p[0] & 0xf) << 2; }
int shl(int *p){ return (p[0] << 2) & 0xff0; }
