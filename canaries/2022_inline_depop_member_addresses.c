// flags: -Cpp_exceptions off -pragma "cats off"
struct Volume { int value; short delta; };
struct Volume output[2];
struct Volume single;
int sum, other;
inline void fade(int *host, int *volume, short *delta_out) {
    int frames;
    int delta;
    frames = *host / 160;
    if (frames) {
        delta = *host / 160;
        if (delta > 20) delta = 20;
        if (delta < -20) delta = -20;
        *volume = *host;
        *host -= delta * 160;
        *delta_out = -delta;
        return;
    }
    *host = 0;
    *volume = 0;
    *delta_out = 0;
}
void update(void) { fade(&sum, (void *)&single.value, &single.delta); }
void update_pair(void) {
    fade(&sum, (void *)&output[0].value, &output[0].delta);
    fade(&other, (void *)&output[1].value, &output[1].delta);
}
