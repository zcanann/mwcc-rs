// An anonymous bit-field `type : width;` is padding with no member — common in
// hardware-register and packed structs. parse_struct_body parsed a field name first
// and skipped the whole struct on the nameless `:`. Now a positive width advances
// the allocation unit (same packing as a named bit-field) and a zero width `: 0`
// closes the open unit so the next bit-field starts at the next boundary.
struct Flags {
    unsigned enable : 1;
    unsigned        : 3;   // padding
    unsigned mode   : 4;
    unsigned        : 0;   // align next to a new unit
    unsigned tag    : 8;
};
int get_mode(struct Flags *p) { return p->mode; }
int get_tag(struct Flags *p)  { return p->tag; }
