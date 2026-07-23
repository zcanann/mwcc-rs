typedef struct Value {
    float member;
} Value;

int product_is_nonnegative(Value* value, float factor)
{
    if (!(value->member * factor < 0)) {
        return 1;
    }
    return 0;
}
