// Opaque wide values: definition lifetimes, pair argument permutations and returns.
typedef unsigned long long u64;
extern u64 first_value(void);
extern u64 second_value(void);
extern u64 transform(u64);
extern void consume(u64);
extern void consume_pair(u64, u64);
extern void consume_three(u64, u64, u64);
extern void disturb(void);

u64 retained_return(void) {
    u64 value = first_value();
    disturb();
    return value;
}
u64 transformed_return(void) {
    u64 value = first_value();
    value = transform(value);
    disturb();
    return value;
}
void pair_permutation(void) {
    u64 first = first_value();
    u64 second = second_value();
    consume_pair(second, first);
}
void repeated_argument(void) {
    u64 value = first_value();
    consume_pair(value, value);
}
void overlapping_values(void) {
    u64 first = first_value();
    u64 second = second_value();
    disturb();
    consume_three(second, first, second);
}
void reused_local(void) {
    u64 value = first_value();
    disturb();
    consume(value);
    value = second_value();
    disturb();
    consume(value);
}
u64 consumed_return(void) {
    u64 value = first_value();
    consume(value);
    return value;
}
