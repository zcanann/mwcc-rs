// A pointer-cast dereference `*(T*)p` — type punning, reinterpreting an address at a chosen width —
// is byte-exact. The load/store type is the cast's target POINTEE (`*(int*)p` -> lwz, `*(short*)p` ->
// lha, `*(char*)p` -> lbz); the address is a leaf pointer operand whose own pointee (`void*`, `char*`)
// is irrelevant. Works for load, store, and a constant subscript (`((int*)p)[1]`), all int widths.
//
// (Subtlety: pointee_of_type(Type::Pointer(_)) returns UnsignedInt — a pointer value is a 4-byte word
// — so the load type is destructured from `Type::Pointer(pointee)`, not derived via pointee_of_type,
// which would make every cast-deref a full-word load.)
int      pun_int  (void *p)         { return *(int*)p;      }  // lwz r3,0(r3)
short    pun_short(void *p)         { return *(short*)p;    }  // lha r3,0(r3)
char     pun_char (void *p)         { return *(char*)p;     }  // lbz r3,0(r3)
unsigned pun_uint (char *p)         { return *(unsigned*)p; }  // lwz (reinterpret char* as unsigned*)
void     pun_store(void *p, int v)  { *(int*)p = v;         }  // stw r4,0(r3)
int      pun_off  (char *p)         { return ((int*)p)[1];  }  // lwz r3,4(r3)
