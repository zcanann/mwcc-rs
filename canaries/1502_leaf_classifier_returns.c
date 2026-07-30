// builds: GC/1.2.5n
// flags: -O4,p -inline auto -Cpp_exceptions off -pragma "cats off"

typedef unsigned int u32;

typedef struct Command {
    u32 pad;
    u32 state;
    u32 command;
} Command;

extern u32 last_error;
extern u32 retry_count;
extern Command* executing;

u32 leaf_classifier_returns(u32 error)
{
    if (error == 0x20400) {
        last_error = error;
        return 1;
    }

    error &= 0x00FFFFFF;
    if (error == 0x62800 || error == 0x23A00 || error == 0xB5A01) {
        return 0;
    }

    retry_count++;
    if (retry_count == 2) {
        if (error == last_error) {
            last_error = error;
            return 1;
        }
        last_error = error;
        return 2;
    }

    last_error = error;
    if (error == 0x31100 || executing->command == 5) {
        return 2;
    }
    return 3;
}
