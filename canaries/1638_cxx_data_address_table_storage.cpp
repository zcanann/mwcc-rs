// C++ internal and exported const address tables.
// flags: -Cpp_exceptions off -pragma "cats off"
int data[8]={1};
static int* mutable1[1]={data};
static int* mutable2[2]={data,&data[3]};
static int* mutable3[3]={data,&data[3],data};
static int* mutable4[4]={data,&data[3],data,&data[3]};
static int* mutable8[8]={data,&data[3],data,&data[3],data,&data[3],data,&data[3]};
static int* const internal1[1]={data};
static int* const internal2[2]={data,&data[3]};
static int* const internal3[3]={data,&data[3],data};
static int* const internal4[4]={data,&data[3],data,&data[3]};
static int* const internal8[8]={data,&data[3],data,&data[3],data,&data[3],data,&data[3]};
extern int* const exported1[1]={data};
extern int* const exported2[2]={data,&data[3]};
extern int* const exported3[3]={data,&data[3],data};
extern int* const exported4[4]={data,&data[3],data,&data[3]};
extern int* const exported8[8]={data,&data[3],data,&data[3],data,&data[3],data,&data[3]};
