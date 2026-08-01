// builds: GC/1.3.2
// flags: -O0,p -Cpp_exceptions off -sdata 0 -sdata2 0 -pool off

typedef signed char s8;
typedef unsigned char u8;

typedef struct NarrowRecord {
    u8 unsigned_value;
    u8 first_zero;
    u8 second_zero;
    s8 signed_value;
} NarrowRecord;

void store_unsigned_member(NarrowRecord* record, int wide)
{
    record->unsigned_value = wide;
}

void store_signed_member(NarrowRecord* record, int wide)
{
    record->signed_value = wide;
}

void store_unsigned_sum(NarrowRecord* record, int wide)
{
    record->unsigned_value = wide + 1;
}

void store_signed_sum(NarrowRecord* record, int wide)
{
    record->signed_value = wide + 1;
}

void store_chained_zero(NarrowRecord* record)
{
    record->first_zero = record->second_zero = 0;
}
