// flags: -Cpp_exceptions off -pragma "cats off" -schedule off
int input;
extern void mutate(int value);
void retained(short *out, int step, int a, int b, int c, int *after) { input -= step * 160; *out = step * -1; *after = a + b + c; }
void late_input(short *out, int step, int *after) { input -= step * 160; *out = step * -1; *after = step + 17; }
void byte_result(signed char *out, int step, int a, int *after) { input -= step * 160; *out = step * -1; *after = a; }
void two_updates(short *out, int first, int second) { input -= first * 160; out[0] = first * -1; input -= second * 160; out[1] = second * -1; }
void framed(short *out, int step, int value) { input -= step * 160; *out = step * -1; mutate(value); }
void volume_first(int *volume, int step, short *out, int value, int *after) { *volume = input; input -= step * 160; *out = step * -1; *after = value; }
