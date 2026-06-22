// `volatile` is transparent to struct layout and to a simple (un-elided) access, so
// a struct with a volatile member records its layout and its members resolve — the
// CARDControl `vu32 mode;` shape that used to skip the whole struct ("struct not
// declared"). A volatile member read/write is a plain lwz/stw, byte-identical to a
// non-volatile one. (A volatile LOCAL is deferred: its access must not be folded.)
typedef volatile unsigned int vu32;
struct Hw { int id; vu32 status; volatile int ctrl; };
int  read_id(struct Hw *p)        { return p->id; }
int  read_status(struct Hw *p)    { return p->status; }
void set_ctrl(struct Hw *p, int v){ p->ctrl = v; }
