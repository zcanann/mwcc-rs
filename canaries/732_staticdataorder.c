// `static` (file-local) data symbols: the INITIALIZED ones (.sdata) come first in
// forward declaration order, then the ZERO ones (.sbss) in REVERSE — mwcc orders
// `static int a;b;c;` (uninitialized) as symbols `c b a`, but `static int a=1;b=2;`
// (initialized) as `a b`.
static int sdo_init1 = 1;
static int sdo_zero1;
static int sdo_init2 = 3;
static int sdo_zero2;
int sdo_global;
