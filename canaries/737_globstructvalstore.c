// Storing a member of a SMALL (<= 8 byte, SDA-addressed) global struct VALUE:
// mwcc materializes the stored VALUE first, then the base. An offset-0 store
// folds the SDA21 into the store (`li r0,v; stw r0,g@sda21`); a non-zero offset
// materializes g's SDA base after the value (`li r0,v; li r3,g@sda21;
// stw r0,off(r3)`). Constant and register values, word and short members.
struct Gsv { int head; short tag; };
struct Gsv gsv;
void gsv_set_head_const(void) { gsv.head = 5; }
void gsv_set_head_reg(int x)  { gsv.head = x; }
void gsv_set_tag_const(void)  { gsv.tag = 7; }
void gsv_set_tag_reg(int x)   { gsv.tag = x; }
