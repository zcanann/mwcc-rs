// MSL_C misc_io.c: assign a function's address to a function-pointer global.
// mwcc materializes the function address absolutely (lis t,@ha; addi r0,t,@lo)
// even with small-data on, then stores it via SDA21. Reloc-exact (the external
// symbol order is mwcc's tabled tree-walk order, not .text-reference order).
extern void (*__stdio_exit)(void);
extern void __close_all(void);
void __stdio_atexit(void) { __stdio_exit = __close_all; }
