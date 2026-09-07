// Const function-pointer typedefs retain read-only table storage.
// flags: -Cpp_exceptions off -pragma "cats off"
typedef void (*Callback)(void);
extern void first(void);
extern void second(void);
static Callback const table1[1]={first};
static Callback const table2[2]={first,second};
static Callback const table3[3]={first,second,first};
static Callback const table4[4]={first,second,first,second};
static Callback const table8[8]={first,second,first,second,first,second,first,second};
static const Callback prefix_const[2]={first,second};
