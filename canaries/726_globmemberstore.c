// Storing a member through a GLOBAL struct base: materialize the base (a struct
// POINTER's value via `lwz`, or a struct VALUE's address via `li`/`lis;addi`)
// into a register chosen to avoid the value's inputs, then a displacement store
// at the member offset. Register and constant values; word and short members.
// (Array-member stores `arr[i].f = v` still defer — the interleaved schedule.)
struct Gms { int first; int second; short tag; };
struct Gms *gms_ptr;
struct Gms gms_val;
void gms_set_ptr(int x)  { gms_ptr->second = x; }
void gms_set_const(void) { gms_ptr->second = 5; }
void gms_set_val(int x)  { gms_val.second = x; }
void gms_set_short(int x){ gms_ptr->tag = x; }
