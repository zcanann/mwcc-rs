// Large zero `.bss` globals lay out in SYMBOL-EMISSION order, not declaration
// order: referenced ones first (in reference order), then unreferenced in REVERSE
// declaration order. Two unreferenced large arrays `big1,big2` thus land big2@0,
// big1 next; a referenced one jumps to the front.
int bssorder_a[16];
int bssorder_b[16];
int bssorder_c[16];
int bssorder_use(int i) { return bssorder_b[i]; }
