// A local array is frame-resident: its storage is a slot of N * sizeof(element)
// bytes above the linkage area, and the bare array name decays to that slot's
// address (`addi d,r1,offset`) — e.g. passing it to a function. The element's true
// width sizes the slot (1 per char, not the 4-byte scalar spill).
void take_int(int *);
void take_char(char *);
void take_float(float *);
void pass_int_buf(void)   { int   buf[4];  take_int(buf); }
void pass_char_buf(void)  { char  buf[16]; take_char(buf); }
void pass_float_buf(void) { float buf[4];  take_float(buf); }
void pass_odd_buf(void)   { char  buf[3];  take_char(buf); }
