// A switch case label is parsed as a full constant expression, so an enum
// constant resolves (not just a bare integer literal). Contiguous enum values
// keep the switch within the supported (non-jump-table) shape. A negative
// literal label still folds too.
enum Mode { MODE_A, MODE_B, MODE_C };
int enumcase_pick(int x) {
    switch (x) {
        case MODE_A: return 10;
        case MODE_B: return 20;
        case MODE_C: return 30;
        default: return 0;
    }
}
