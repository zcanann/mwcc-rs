// Both operand orders for add/subtract/multiply/and/or/xor.
// flags: -Cpp_exceptions off -pragma "cats off"
struct S{unsigned pad[8],index,value;}; extern unsigned global[128],other[128],extra;
unsigned add0(struct S* object,unsigned* table){return (table[object->index&3])+(table[0]);}
unsigned sub0(struct S* object,unsigned* table){return (table[object->index&3])-(table[0]);}
unsigned mul0(struct S* object,unsigned* table){return (table[object->index&3])*(table[0]);}
unsigned and0(struct S* object,unsigned* table){return (table[object->index&3])&(table[0]);}
unsigned or0(struct S* object,unsigned* table){return (table[object->index&3])|(table[0]);}
unsigned xor0(struct S* object,unsigned* table){return (table[object->index&3])^(table[0]);}
unsigned add1(struct S* object,unsigned* table){return (table[0])+(table[object->index&3]);}
unsigned sub1(struct S* object,unsigned* table){return (table[0])-(table[object->index&3]);}
unsigned mul1(struct S* object,unsigned* table){return (table[0])*(table[object->index&3]);}
unsigned and1(struct S* object,unsigned* table){return (table[0])&(table[object->index&3]);}
unsigned or1(struct S* object,unsigned* table){return (table[0])|(table[object->index&3]);}
unsigned xor1(struct S* object,unsigned* table){return (table[0])^(table[object->index&3]);}
