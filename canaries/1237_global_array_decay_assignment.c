// flags: -O3,s -sdata 0 -sdata2 0

extern int first;
extern int second;

int *profile_list[] = {&first, &second, 0};
int **active_profiles;

void install_profiles(void) {
    active_profiles = profile_list;
}
