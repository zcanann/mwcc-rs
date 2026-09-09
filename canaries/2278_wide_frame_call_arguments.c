// Retained frame pairs forwarded through ordinary calls and control flow.
typedef unsigned long long u64;
typedef long long s64;
extern u64 first_value(void);
extern u64 second_value(void);
extern s64 signed_value(void);
extern void consume(u64 value);
extern void consume_signed(s64 value);
extern void consume_pair(u64 first, u64 second);
extern void consume_mixed(unsigned key, u64 value, unsigned tail);
extern void disturb(void);
void across_call(void) { u64 value=first_value(); disturb(); consume(value); }
void repeated(void) { u64 value=first_value(); consume(value); disturb(); consume(value); }
void two_values(void) { u64 first=first_value(); u64 second=second_value(); consume_pair(first,second); }
void mixed_arguments(unsigned key, unsigned tail) { u64 value=first_value(); consume_mixed(key,value,tail); }
void signed_argument(void) { s64 value=signed_value(); disturb(); consume_signed(value); }
void conditional(unsigned flag) { u64 value=first_value(); if (flag) consume(value); }
