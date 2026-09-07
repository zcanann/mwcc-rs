// Array size affects object alignment independently of element alignment.
// flags: -Cpp_exceptions off -pragma "cats off"
int lead = 1;
char block8[8] = {1};
int middle = 2;
char block9[9] = {1};
char block16[16] = {1};
int end = 3;
const short read_only8[4] = {1};
const char read_only9[9] = {1};
char aligned16[16] __attribute__((aligned(32))) = {1};
