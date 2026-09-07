// One biased index paired with a plain masked index; captured on fifteen builds.
// flags: -Cpp_exceptions off -pragma "cats off"
struct S{unsigned pad[8],index,value;}; extern unsigned global[128],other[128],extra;
unsigned add0(unsigned index,unsigned* table){return (table[index&3])+(table[(index+1)&3]);}
unsigned sub0(unsigned index,unsigned* table){return (table[index&3])-(table[(index+1)&3]);}
unsigned mul0(unsigned index,unsigned* table){return (table[index&3])*(table[(index+1)&3]);}
unsigned and0(unsigned index,unsigned* table){return (table[index&3])&(table[(index+1)&3]);}
unsigned or0(unsigned index,unsigned* table){return (table[index&3])|(table[(index+1)&3]);}
unsigned xor0(unsigned index,unsigned* table){return (table[index&3])^(table[(index+1)&3]);}
unsigned add1(unsigned index,unsigned* table){return (table[(index+1)&3])+(table[index&3]);}
unsigned sub1(unsigned index,unsigned* table){return (table[(index+1)&3])-(table[index&3]);}
unsigned mul1(unsigned index,unsigned* table){return (table[(index+1)&3])*(table[index&3]);}
unsigned and1(unsigned index,unsigned* table){return (table[(index+1)&3])&(table[index&3]);}
unsigned or1(unsigned index,unsigned* table){return (table[(index+1)&3])|(table[index&3]);}
unsigned xor1(unsigned index,unsigned* table){return (table[(index+1)&3])^(table[index&3]);}
