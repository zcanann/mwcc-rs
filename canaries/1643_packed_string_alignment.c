// Packed literals form one byte-array object whose total size controls alignment.
// flags: -Cpp_exceptions off -pragma "cats off" -O0 -str pool
char lead[3]={1};
char* first="1234567";
char* second="abcdefg";
