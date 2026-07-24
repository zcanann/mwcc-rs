// builds: GC/1.2.5n
// flags: -O4,p -inline auto -Cpp_exceptions off -pragma "cats off"

struct Fighter {
    char pad0[12];
    unsigned char player;
    char pad1[1555];
    float stick_x;
    float stick_y;
    char pad2[40];
    float cstick_x;
    char pad3[37];
    unsigned char count_x;
    unsigned char count_y;
    unsigned char count_c;
    char pad4[7075];
    unsigned char flag0 : 1;
    unsigned char flag1 : 1;
    unsigned char flag2 : 1;
    unsigned char flag3 : 1;
    unsigned char flag4 : 1;
    unsigned char flag5 : 1;
    unsigned char flag6 : 1;
    unsigned char flag7 : 1;
};

struct Object {
    char pad[44];
    struct Fighter* user_data;
};

struct Limits {
    char pad[1976];
    float stick_threshold;
    float cstick_threshold;
    float count_limit;
};

extern struct Limits* limits;
extern void update_count(int player, int index);

void update_joystick_counts(struct Object* object)
{
    struct Fighter* fighter = object->user_data;
    if (((fighter->stick_x < 0 ? -fighter->stick_x : fighter->stick_x) >=
             limits->stick_threshold &&
         fighter->count_x < limits->count_limit) ||
        ((fighter->stick_y < 0 ? -fighter->stick_y : fighter->stick_y) >=
             limits->stick_threshold &&
         fighter->count_y < limits->count_limit)) {
        update_count((int) fighter->player, fighter->flag4);
        fighter->count_y = 254;
        fighter->count_x = 254;
    }
    if ((fighter->cstick_x < 0 ? -fighter->cstick_x : fighter->cstick_x) >=
        limits->cstick_threshold) {
        if (fighter->count_c < limits->count_limit) {
            update_count((int) fighter->player, fighter->flag4);
            fighter->count_c = 254;
        }
    }
}
