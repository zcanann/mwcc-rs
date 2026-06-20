// A function reading a `static` scalar: the reference relocation resolves to the
// static's LOCAL symbol (not a global/undefined external). The static is declared
// before the function (the supported source order).
static int staticref_value = 5;
int staticref(void) { return staticref_value; }
