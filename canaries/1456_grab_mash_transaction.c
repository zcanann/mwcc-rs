typedef int bool;
typedef signed char s8;

struct Input {
    float stick_x;
    float stick_y;
    char pad[64];
    unsigned int bits;
};

struct Fighter {
    char pad0[12];
    unsigned char player;
    char pad1[1555];
    struct Input input;
    char pad3[5088];
    float timer;
    s8 previous_x;
    s8 previous_y;
    unsigned char count;
    unsigned char limit;
    char pad4[995];
    unsigned char report0 : 1;
    unsigned char report1 : 1;
    unsigned char report2 : 1;
    unsigned char report3 : 1;
    unsigned char report : 1;
    char pad5[1004];
    unsigned char state0 : 1;
    unsigned char state1 : 1;
    unsigned char state2 : 1;
    unsigned char state3 : 1;
    unsigned char state4 : 1;
    unsigned char active : 1;
    unsigned char enabled : 1;
};

struct CommonData {
    char pad[776];
    float threshold;
};

extern struct CommonData* p_ftCommonData;
extern void report_state(unsigned char, int, bool);

bool grab_mash(struct Fighter* fighter, float amount)
{
    bool result = 0;
    if (fighter->input.bits & 0x80000f00u) {
        fighter->timer -= amount;
        result = 1;
    }
    {
        s8 previous_x = fighter->previous_x;
        s8 previous_y = fighter->previous_y;
        if (fighter->input.stick_x < -p_ftCommonData->threshold) {
            fighter->previous_x = -1;
        }
        if (fighter->input.stick_x > p_ftCommonData->threshold) {
            fighter->previous_x = 1;
        }
        if (fighter->input.stick_y < -p_ftCommonData->threshold) {
            fighter->previous_y = -1;
        }
        if (fighter->input.stick_y > p_ftCommonData->threshold) {
            fighter->previous_y = 1;
        }
        if (previous_x != fighter->previous_x ||
            previous_y != fighter->previous_y) {
            fighter->timer -= amount;
            result = 1;
        }
    }
    if (result && fighter->enabled) {
        fighter->active = 1;
        fighter->count += 1;
        if (fighter->count >= fighter->limit) {
            fighter->count = 0;
        }
    } else {
        fighter->active = 0;
    }
    report_state(fighter->player, fighter->report, result);
    return result;
}
