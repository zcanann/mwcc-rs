// Masked global-array read combined with another global in both operand orders.
// flags: -Cpp_exceptions off -pragma "cats off" -O0
struct S{unsigned pad[8],index,value;}; extern unsigned global[128],other[128],extra;
unsigned add0(unsigned index){return (global[index&3])+(extra);}
unsigned sub0(unsigned index){return (global[index&3])-(extra);}
unsigned mul0(unsigned index){return (global[index&3])*(extra);}
unsigned and0(unsigned index){return (global[index&3])&(extra);}
unsigned or0(unsigned index){return (global[index&3])|(extra);}
unsigned xor0(unsigned index){return (global[index&3])^(extra);}
unsigned add1(unsigned index){return (extra)+(global[index&3]);}
unsigned sub1(unsigned index){return (extra)-(global[index&3]);}
unsigned mul1(unsigned index){return (extra)*(global[index&3]);}
unsigned and1(unsigned index){return (extra)&(global[index&3]);}
unsigned or1(unsigned index){return (extra)|(global[index&3]);}
unsigned xor1(unsigned index){return (extra)^(global[index&3]);}
