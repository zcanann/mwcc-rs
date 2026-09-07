// The extern reference stays named; the later initializer can use an anchor.
// flags: -Cpp_exceptions off -pragma "cats off"
extern char target[13];
char* before_pointer = target;
char target[13];
char* after_pointer = target;
