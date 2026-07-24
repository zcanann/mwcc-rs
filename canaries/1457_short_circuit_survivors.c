typedef unsigned int u32;

struct Fighter {
    unsigned char pad0[12];
    unsigned char player_id;
    unsigned char pad1[0x618 - 13];
    unsigned char rumble_id;
    unsigned char pad2[0x221f - 0x619];
    unsigned char x221f_b0 : 1;
    unsigned char x221f_b1 : 1;
    unsigned char x221f_b2 : 1;
    unsigned char x221f_b3 : 1;
    unsigned char x221f_b4 : 1;
    unsigned char x221f_b5 : 1;
    unsigned char x221f_b6 : 1;
    unsigned char x221f_b7 : 1;
    unsigned char pad3[4];
    unsigned char x2224_b0 : 1;
    unsigned char x2224_b1 : 1;
    unsigned char x2224_b2 : 1;
    unsigned char x2224_b3 : 1;
    unsigned char x2224_b4 : 1;
    unsigned char x2224_b5 : 1;
    unsigned char x2224_b6 : 1;
    unsigned char x2224_b7 : 1;
};

extern int player_is_active(unsigned char, unsigned char);
extern void emit_rumble(unsigned char, u32, u32, u32);

void short_circuit_survivors(struct Fighter* fighter, u32 duration, u32 strength)
{
    if (player_is_active(fighter->player_id, fighter->x221f_b4) &&
        !fighter->x221f_b3 && !fighter->x2224_b2) {
        emit_rumble(fighter->rumble_id, duration + 2, duration, strength);
    }
}
