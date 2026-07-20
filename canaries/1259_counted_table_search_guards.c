// flags: -char unsigned -sdata 0 -sdata2 0 -O4,s
// builds: GC/1.3.2 GC/2.6
extern int check_status(int event, int mask);

static unsigned char event_table[4] = { 1, 2, 3, 4 };

unsigned char find_event(void) {
    unsigned char* event_table_p = event_table;
    int i = 0;
    for (i = 0; i < 4; i++) {
        if (i != 2 && check_status(event_table[i], 16) != 0) {
            return i;
        }
    }
    if (check_status(9, 16)) {
        return 2;
    }
    if (check_status(10, 16)) {
        return 5;
    }
    return 255;
}
