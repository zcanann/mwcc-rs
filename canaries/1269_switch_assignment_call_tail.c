// builds: GC/1.3 GC/1.3.2 GC/2.0 GC/2.0p1 GC/2.6 GC/2.7
// flags: -sdata 0 -O0,p -Cpp_exceptions off
short selector;

void create_window(int position, int message, int portrait);
void wait_window(void);
void kill_window(void);

void run_window(int message) {
    short position;
    switch (selector) {
    case 0:
        position = 5;
        break;
    case 1:
        position = 6;
        break;
    }
    create_window(position, message, -1);
    wait_window();
    kill_window();
}
