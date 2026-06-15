// flags: -sdata 0 -sdata2 0
// A float global: the address base is a separate free GPR, @l folded into lfs.
extern float g;
float absfread(void){ return g; }
