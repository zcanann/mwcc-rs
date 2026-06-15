// Multiple initialized scalars land in .sdata in FORWARD declaration order
// (a@0, b@4, c@8); reading b loads from offset 4.
int a = 11;
int b = 22;
int c = 33;
int getb(void){ return b; }
