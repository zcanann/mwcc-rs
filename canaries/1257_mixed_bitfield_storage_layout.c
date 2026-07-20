typedef struct MixedBits {
    unsigned short year : 12;
    unsigned short month : 4;
    unsigned char day : 5;
    unsigned char day_pad : 3;
    unsigned char hour : 5;
    unsigned char hour_pad : 3;
    unsigned char quarter : 4;
    unsigned char active : 1;
    unsigned char final_pad : 3;
    unsigned char end;
} MixedBits;

unsigned char read_after_mixed_bitfields(MixedBits* value) {
    return value->end;
}
