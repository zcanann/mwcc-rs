/* Computed truth tests must preserve their default and shared inputs. */
void leap_year(int year, int *out) {
    *out = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
}
void quotient_and(int left, int right, int *out) {
    *out = (left / 7 == 3) && (right % 11 == 5);
}
void quotient_or(int left, int right, int *out) {
    *out = (left / 7 == 3) || (right % 11 == 5);
}
void shared_input(int value, int *out) {
    *out = value % 4 == 0 && value % 100 != 0;
}
void guarded_read(int flag, volatile int *value, int *out) {
    *out = flag && (*value % 11 == 0);
}
