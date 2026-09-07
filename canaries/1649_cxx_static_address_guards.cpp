// First-use guards preserve symbolic base construction and typed byte offsets.
// flags: -Cpp_exceptions off -pragma "cats off"
int data[8]={1};
int small=1;
int* array_pointer() { static int* p=&data[3]; return p; }
int* word_pointer() { static int* p=&small; return p; }
int* integer_pointer() { static int* p=(int*)0x1000; return p; }
int* null_pointer() { static int* p=0; return p; }
