// An empty statement (a lone `;`) is a no-op: it produces no code. Common as the
// body of a busy-wait loop (`while (cond) ;`) or a guarded no-op. Here lone
// semicolons surround a real statement and contribute nothing to the output.
int emptystmt(int x) {
    ;
    ;
    return x + 1;
}
