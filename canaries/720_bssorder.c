// Section layout order for uninitialized globals differs by small-data class:
// large .bss objects lay out FORWARD (declaration order), but small .sbss objects
// lay out REVERSE. Two large arrays here exercise .bss forward order (regression
// guard: we used to reverse .bss too, which mis-placed the referenced array).
int bssorder_first[8];
int bssorder_second[32];
int bssorder_get(int i) { return bssorder_first[i]; }
