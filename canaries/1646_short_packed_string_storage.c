// A packed pool uses full data storage even when its total size is eight bytes.
// flags: -Cpp_exceptions off -pragma "cats off" -O0 -str pool
char lead[3]={1};
char* text="1234567";
