// Counterpart to 720: small uninitialized globals (.sbss, <= 8 bytes) lay out in
// REVERSE declaration order (the referenced scalar lands at the higher offset).
int sbssorder_a;
int sbssorder_b;
int sbssorder_get(void) { return sbssorder_a; }
