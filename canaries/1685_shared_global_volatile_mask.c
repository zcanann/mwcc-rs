// Candidate execution probe: volatile word reads with a discontiguous mask.
// Fresh reference comparison pending; the captured pair uses mask 3.
// flags: -Cpp_exceptions off -pragma "cats off"
extern volatile unsigned global[128];
unsigned add0(unsigned index){return (global[index&85])+(global[0]);}
unsigned sub0(unsigned index){return (global[index&85])-(global[0]);}
unsigned mul0(unsigned index){return (global[index&85])*(global[0]);}
unsigned and0(unsigned index){return (global[index&85])&(global[0]);}
unsigned or0(unsigned index){return (global[index&85])|(global[0]);}
unsigned xor0(unsigned index){return (global[index&85])^(global[0]);}
unsigned add1(unsigned index){return (global[0])+(global[index&85]);}
unsigned sub1(unsigned index){return (global[0])-(global[index&85]);}
unsigned mul1(unsigned index){return (global[0])*(global[index&85]);}
unsigned and1(unsigned index){return (global[0])&(global[index&85]);}
unsigned or1(unsigned index){return (global[0])|(global[index&85]);}
unsigned xor1(unsigned index){return (global[0])^(global[index&85]);}
