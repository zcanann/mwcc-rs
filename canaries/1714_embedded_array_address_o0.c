// Candidate execution probe; fresh compiler-reference objects pending.
// flags: -O0 -Cpp_exceptions off -pragma "cats off"
// AX's &pvpb->updateData is an address-of an embedded array.
struct Block { unsigned pad[13]; unsigned short data[8]; };
extern struct Block block;
extern void fill(unsigned short*);
unsigned short* pointer_array(struct Block* p) { return (void*)&p->data; }
unsigned short* global_array(void) { return (void*)&block.data; }
void local_array(void) {
    struct Block local;
    fill((void*)&local.data);
}
unsigned short* nested_array(struct Block** p) { return (void*)&(*p)->data; }
