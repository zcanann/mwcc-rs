// Four incoming values survive a status call and feed a guarded aggregate update.
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

extern s32 get_record(s32 channel, s32 index, Record* record);
extern s32 set_record_async(s32 channel, s32 index, Record* record, s32* callback);

s32 update_record_async(s32 channel, s32 index, u8 attribute, s32* callback) {
    Record record;
    s32 result;

    result = get_record(channel, index, &record);
    if (result < 0) {
        return result;
    }
    record.attribute = attribute;
    return set_record_async(channel, index, &record, callback);
}
