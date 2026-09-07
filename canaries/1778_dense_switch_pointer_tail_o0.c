// Dense dispatch must preserve the pointer used by the continuation.
// flags: -O0 -Cpp_exceptions off -pragma "cats off"
unsigned pick(int id, unsigned* values) {
 unsigned index;
 switch(id) {
 case 0: index=7; break;
 case 1: index=2; break;
 case 2: index=6; break;
 case 3: index=1; break;
 case 4: index=5; break;
 case 5: index=0; break;
 case 6: index=4; break;
 case 7: index=3; break;
 default: index=8; break;
 }
 return values[index];
}
