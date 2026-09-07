// Aggregate addresses retain pointer size and arithmetic stride.
// flags: -Cpp_exceptions off -pragma "cats off"
typedef struct Wide { unsigned a, b, c; } Wide;
unsigned address_size(Wide value) { return sizeof(&value); }
unsigned source_size(Wide value) { return sizeof(value); }
unsigned advance(Wide value) { return (unsigned)(&value + 1) - (unsigned)&value; }
