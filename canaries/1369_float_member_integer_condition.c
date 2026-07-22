// builds: GC/1.2.5n

typedef struct Value {
    float amount;
} Value;

void act(void);

void float_member_integer_condition(Value* value)
{
    if (value->amount < 0) {
        act();
    }
}
