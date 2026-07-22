// builds: GC/1.2.5n

typedef struct Values {
    float floating;
    unsigned char narrow;
    int integer;
} Values;

void act(void);

void loaded_member_conditions(Values* left, Values* right)
{
    if (left->floating >= right->floating && left->narrow < right->integer) {
        act();
        return;
    }
    if (left->integer) {
        act();
    }
}
