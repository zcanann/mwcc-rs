// Rotate inserts mask source bits; C shift/OR updates retain outside bits.
// flags: -O0 -Cpp_exceptions off -pragma "cats off"
extern unsigned __rlwimi(unsigned, unsigned, unsigned, unsigned, unsigned);
struct State { unsigned padding, field, untouched, dirty; };
extern struct State* const state;
typedef struct State* StatePointer;
extern volatile StatePointer changing;
void narrow(unsigned char value) { state->field = __rlwimi(state->field,value,16,13,15); state->dirty |= 6; }
void wrapped(unsigned char value) { state->field = __rlwimi(state->field,value,31,28,3); state->dirty |= 0x8001; }
void full(unsigned char value) { state->field = __rlwimi(state->field,value,7,0,31); state->dirty |= 2; }
void same_word(unsigned char value) { state->field = __rlwimi(state->field,value,0,24,31); state->field |= 0x100; }
void legacy(unsigned char value) { (void)0; state->field = (state->field & 0xfff8ffff) | ((unsigned)value << 16); state->dirty |= 6; }
void truncated_old(unsigned char value) { state->field = __rlwimi((unsigned short)state->field,value,16,13,15); state->dirty |= 6; }
void truncated_result(unsigned char value) { state->field = (unsigned short)__rlwimi(state->field,value,16,13,15); state->dirty |= 6; }
void volatile_base(unsigned char value) { changing->field = __rlwimi(changing->field,value,16,13,15); changing->dirty |= 6; }
