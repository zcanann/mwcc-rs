// builds: GC/1.2.5n

typedef struct Value {
    void* owner;
} Value;

Value* current_value;

float adjustment(void* owner);
int consume(Value* value, float amount);

int float_nested_second_argument(void)
{
    return consume(current_value, adjustment(current_value->owner));
}
