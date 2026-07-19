// flags: -O3,s -schedule off -sdata 0 -sdata2 0

extern int first;
extern int second;

int *profile_list[] = {&first, &second, 0};
int **active_profile;

void install_profile(void) {
    active_profile = profile_list;
}

void clear_profile(void) {
    active_profile = 0;
}
