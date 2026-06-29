// `(*(T *)0xADDR).field` — a member access through a constant-address pointer. This is the GX
// write-gather FIFO shape: `(*(volatile PPCWGPipe *)0xCC008000).u8 = v`. The member offset folds
// into the constant-address displacement (`lis base, hi(ADDR); st v, (lo(ADDR)+offset)(base)`).
// A union member is at offset 0 (identical to a plain `*(T *)ADDR` access); a struct member adds
// its byte offset. Float/double members and an i16-overflowing displacement still defer.
union FIFO { unsigned char b; unsigned short h; unsigned int w; };
void wr_b(unsigned char  v) { (*(volatile union FIFO *)0xCC008000).b = v; }  // lis r4; stb v,lo(r4)
void wr_h(unsigned short v) { (*(volatile union FIFO *)0xCC008000).h = v; }  // lis r4; sth v,lo(r4)
void wr_w(unsigned int   v) { (*(volatile union FIFO *)0xCC008000).w = v; }  // lis r4; stw v,lo(r4)
struct HW { int status; int control; };
int  rd_ctl(void)           { return (*(struct HW *)0xCC000000).control; }   // lis r3; lwz r3,4(r3)
void wr_ctl(int v)          { (*(struct HW *)0xCC000000).control = v; }       // off-4 store
