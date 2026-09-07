// Captured reference: masked and constant subscripts sharing a global-array base.
// flags: -O0 -Cpp_exceptions off -pragma "cats off"
struct S{unsigned pad[8],index,value;}; extern unsigned global[128],other[128],extra;
unsigned add0(unsigned index){return (global[index&3])+(global[0]);}
unsigned sub0(unsigned index){return (global[index&3])-(global[0]);}
unsigned mul0(unsigned index){return (global[index&3])*(global[0]);}
unsigned and0(unsigned index){return (global[index&3])&(global[0]);}
unsigned or0(unsigned index){return (global[index&3])|(global[0]);}
unsigned xor0(unsigned index){return (global[index&3])^(global[0]);}
unsigned add1(unsigned index){return (global[0])+(global[index&3]);}
unsigned sub1(unsigned index){return (global[0])-(global[index&3]);}
unsigned mul1(unsigned index){return (global[0])*(global[index&3]);}
unsigned and1(unsigned index){return (global[0])&(global[index&3]);}
unsigned or1(unsigned index){return (global[0])|(global[index&3]);}
unsigned xor1(unsigned index){return (global[0])^(global[index&3]);}
