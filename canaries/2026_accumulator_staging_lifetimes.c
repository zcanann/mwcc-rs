// flags: -Cpp_exceptions off -pragma "cats off"
struct Packet { int padding[21];
    short field0;
    unsigned short field1;
    short field2;
    unsigned short field3;
    short field4;
    unsigned short field5;
    short field6;
    unsigned short field7;
    short field8;
};
int total0, total1, total2, total3, total4, total5, total6, total7, total8;
void accumulate_2(struct Packet *packet) {
    total0 += packet->field1;
    total1 += packet->field0;
}
void accumulate_3(struct Packet *packet) {
    total0 += packet->field2;
    total1 += packet->field1;
    total2 += packet->field0;
}
void accumulate_4(struct Packet *packet) {
    total0 += packet->field3;
    total1 += packet->field2;
    total2 += packet->field1;
    total3 += packet->field0;
}
void accumulate_5(struct Packet *packet) {
    total0 += packet->field4;
    total1 += packet->field3;
    total2 += packet->field2;
    total3 += packet->field1;
    total4 += packet->field0;
}
void accumulate_6(struct Packet *packet) {
    total0 += packet->field5;
    total1 += packet->field4;
    total2 += packet->field3;
    total3 += packet->field2;
    total4 += packet->field1;
    total5 += packet->field0;
}
void accumulate_7(struct Packet *packet) {
    total0 += packet->field6;
    total1 += packet->field5;
    total2 += packet->field4;
    total3 += packet->field3;
    total4 += packet->field2;
    total5 += packet->field1;
    total6 += packet->field0;
}
void accumulate_8(struct Packet *packet) {
    total0 += packet->field7;
    total1 += packet->field6;
    total2 += packet->field5;
    total3 += packet->field4;
    total4 += packet->field3;
    total5 += packet->field2;
    total6 += packet->field1;
    total7 += packet->field0;
}
void accumulate_9(struct Packet *packet) {
    total0 += packet->field8;
    total1 += packet->field7;
    total2 += packet->field6;
    total3 += packet->field5;
    total4 += packet->field4;
    total5 += packet->field3;
    total6 += packet->field2;
    total7 += packet->field1;
    total8 += packet->field0;
}
