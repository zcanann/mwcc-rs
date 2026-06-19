int loadfield(int *p){ return (p[0] & 0xff00) >> 8; }
unsigned hibyte(unsigned *p){ return (p[0] & 0xff000000) >> 24; }
