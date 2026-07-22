// A call-filled aggregate exposes one byte only on its successful status edge.
// builds: GC/1.2.5n
// flags: -Cpp_exceptions off -O4,p -inline all -sdata 8 -sdata2 8
typedef signed long s32;
typedef unsigned char u8;
typedef unsigned long u32;

typedef struct Record {
    u32 alignment;
    u8 prefix[48];
    u8 attribute;
    u8 suffix[11];
} Record;

extern s32 fill_record(s32 channel, s32 index, Record* record);

s32 copy_record_attribute(s32 channel, s32 index, u8* output) {
    Record record;
    s32 result;

    result = fill_record(channel, index, &record);
    if (result == 0) {
        *output = record.attribute;
    }
    return result;
}
