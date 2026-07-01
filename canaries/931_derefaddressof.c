// `*&x` cancels to `x` — the dereference of an address-of is the operand itself (the C identity),
// folded in the parser when a dereference is built. This fixes a wrong-bytes DIFF (`return *&x;` had
// taken the address of a register value and dereferenced it, diverging from mwcc's plain `blr`) and
// unblocks the nested and store forms. The reverse, `&*p` -> `p`, is already handled.
int  deref_addr     (int x)          { return *&x;    }  // blr  (x already in r3, nothing to do)
int  deref2_addr    (int *p)         { return **&p;   }  // lwz r3,0(r3)   (= *p)
int  deref_addr_idx (int *a, int i)  { return *&a[i]; }  // slwi/lwzx      (= a[i])
void store_addr     (int *p, int v)  { *&*p = v;      }  // stw r4,0(r3)   (= *p = v)
