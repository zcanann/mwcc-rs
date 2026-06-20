// Reading a member through a GLOBAL struct pointer: load the pointer value via
// its global addressing, then load the field at its offset — `lwz d, gp@…;
// lwz d, offset(d)`. (The marioparty4 game code threads global state structs
// through file-scope pointers like this.) Word and short members covered; struct
// value/array bases and stores still defer (no miscompile).
struct GlobPtrMember { int first; int second; short tag; };
struct GlobPtrMember *globptr_state;
int globptr_second(void) { return globptr_state->second; }
int globptr_first(void) { return globptr_state->first; }
int globptr_tag(void) { return globptr_state->tag; }
