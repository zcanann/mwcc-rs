// Struct-array strides and member byte offsets compose into one relocation.
// flags: -Cpp_exceptions off -pragma "cats off"
struct Record { char first; int second; short third; };
Record records[4];
Record* record_pointer = &records[2];
int* member_pointer = &records[2].second;
short* other_pointer = &records[1].third;
