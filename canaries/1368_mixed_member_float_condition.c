// builds: GC/1.2.5n

typedef struct Values {
    float floating;
    unsigned char narrow;
} Values;

void act(void);

void mixed_member_float_condition(Values* left, Values* right)
{
    if (left->narrow < right->floating) {
        act();
    }
}
