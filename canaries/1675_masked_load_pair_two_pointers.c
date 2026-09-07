// Frontier: two computed pointer subscripts in one binary expression.
// flags: -Cpp_exceptions off -pragma "cats off"
struct S{unsigned pad[8],index,value;}; extern unsigned global[128],other[128],extra;
unsigned add0(unsigned index,unsigned* table,unsigned* other){return (table[index&3])+(other[index&1]);}
unsigned sub0(unsigned index,unsigned* table,unsigned* other){return (table[index&3])-(other[index&1]);}
unsigned mul0(unsigned index,unsigned* table,unsigned* other){return (table[index&3])*(other[index&1]);}
unsigned and0(unsigned index,unsigned* table,unsigned* other){return (table[index&3])&(other[index&1]);}
unsigned or0(unsigned index,unsigned* table,unsigned* other){return (table[index&3])|(other[index&1]);}
unsigned xor0(unsigned index,unsigned* table,unsigned* other){return (table[index&3])^(other[index&1]);}
unsigned add1(unsigned index,unsigned* table,unsigned* other){return (other[index&1])+(table[index&3]);}
unsigned sub1(unsigned index,unsigned* table,unsigned* other){return (other[index&1])-(table[index&3]);}
unsigned mul1(unsigned index,unsigned* table,unsigned* other){return (other[index&1])*(table[index&3]);}
unsigned and1(unsigned index,unsigned* table,unsigned* other){return (other[index&1])&(table[index&3]);}
unsigned or1(unsigned index,unsigned* table,unsigned* other){return (other[index&1])|(table[index&3]);}
unsigned xor1(unsigned index,unsigned* table,unsigned* other){return (other[index&1])^(table[index&3]);}
