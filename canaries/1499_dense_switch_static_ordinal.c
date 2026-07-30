// builds: GC/1.2.5n
// flags: -O4,p -inline auto -Cpp_exceptions off -pragma "cats off"

typedef unsigned int u32;
extern void fallback(void);
extern void command0(void);
extern void command1(void);
extern void command2(void);
extern void command3(void);
extern void command4(void);
extern void command5(void);
extern void command6(void);

void dense_switch_static_ordinal(u32 command)
{
    switch (command) {
    case 0:
        command0();
        return;
    case 1:
        command1();
        return;
    case 2:
        command2();
        return;
    case 3:
        command3();
        return;
    case 4:
        command4();
        return;
    case 5:
        command5();
        return;
    case 6:
        command6();
        return;
    default:
        fallback();
        return;
    }
}

extern u32 command_log[4];

void record_dense_command(u32 command)
{
    static u32 count;
    command_log[count++] = command;
}
