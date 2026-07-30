// builds: GC/1.2.5n
// flags: -O4,p -inline auto -Cpp_exceptions off -pragma "cats off"

typedef unsigned int u32;

typedef struct CommandBlock {
    u32 pad0;
    u32 pad1;
    u32 pad2;
    int state;
} CommandBlock;

extern int fatal_error;
extern int pausing;
extern CommandBlock* executing;
extern CommandBlock dummy_command;
extern volatile u32 device_registers[];
extern int resume_from_here;
extern int disable_interrupts(void);
extern void restore_interrupts(int level);

int nested_else_global_switch(void)
{
    int level;
    int result;
    int state;
    u32 cover;

    level = disable_interrupts();

    if (fatal_error) {
        state = -1;
    } else if (pausing) {
        state = 8;
    } else {
        if (executing == 0) {
            state = 0;
        } else if (executing == &dummy_command) {
            state = 0;
        } else {
            state = executing->state;
        }
    }

    switch (state) {
    case 1:
    case 2:
    case 3:
    case 4:
        result = 1;
        break;
    case -1:
    case 5:
    case 6:
    case 7:
    case 9:
    case 10:
        result = 0;
        break;
    case 0:
    case 8:
    case 11:
        cover = device_registers[1];
        if (((cover >> 2) & 1) || (cover & 1)) {
            result = 0;
        } else if (resume_from_here != 0) {
            result = 0;
        } else {
            result = 1;
        }
        break;
    }

    restore_interrupts(level);
    return result;
}
