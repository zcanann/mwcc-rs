// flags: -char unsigned -sdata 0 -sdata2 0 -O4,s
// builds: GC/1.3.2 GC/2.6
extern int check_status(int event, int mask);

unsigned char field_event(void) {
    if (check_status(15, 16)) {
        return 0;
    }
    if (check_status(12, 16)) {
        return 1;
    }
    if (check_status(14, 16)) {
        return 2;
    }
    return 3;
}
