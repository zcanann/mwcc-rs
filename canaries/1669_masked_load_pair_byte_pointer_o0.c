// Both operand orders for add/subtract/multiply/and/or/xor.
// flags: -Cpp_exceptions off -pragma "cats off" -O0
struct S{unsigned pad[8],index,value;}; extern unsigned global[128],other[128],extra;
unsigned add0(unsigned index,unsigned char* table){return (table[index&3])+(table[0]);}
unsigned sub0(unsigned index,unsigned char* table){return (table[index&3])-(table[0]);}
unsigned mul0(unsigned index,unsigned char* table){return (table[index&3])*(table[0]);}
unsigned and0(unsigned index,unsigned char* table){return (table[index&3])&(table[0]);}
unsigned or0(unsigned index,unsigned char* table){return (table[index&3])|(table[0]);}
unsigned xor0(unsigned index,unsigned char* table){return (table[index&3])^(table[0]);}
unsigned add1(unsigned index,unsigned char* table){return (table[0])+(table[index&3]);}
unsigned sub1(unsigned index,unsigned char* table){return (table[0])-(table[index&3]);}
unsigned mul1(unsigned index,unsigned char* table){return (table[0])*(table[index&3]);}
unsigned and1(unsigned index,unsigned char* table){return (table[0])&(table[index&3]);}
unsigned or1(unsigned index,unsigned char* table){return (table[0])|(table[index&3]);}
unsigned xor1(unsigned index,unsigned char* table){return (table[0])^(table[index&3]);}
