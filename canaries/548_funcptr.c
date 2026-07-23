// MSL_C misc_io.c: assign a function's address to a function-pointer global.
// mwcc materializes the function address absolutely even with small-data on,
// then stores it via SDA21. GC 1.x/2.x complete the address in r0; GC 3/Wii
// complete and store it from the high-half register. Symbol discovery also
// changes from the older tree walk to later relocation-emission order.
extern void (*__stdio_exit)(void);
extern void __close_all(void);
void __stdio_atexit(void) { __stdio_exit = __close_all; }
