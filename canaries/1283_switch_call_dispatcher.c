// A dense call dispatcher preserves its message pointer and accumulated result
// across prefix/suffix trace calls and every jump-table arm.
// builds: GC/3.0a3p1
// flags: -Cpp_exceptions off -O4,p -inline deferred,auto -rostr -str reuse -sdata 0 -sdata2 0
typedef struct DispatchMessage {
    unsigned char bytes[21];
} DispatchMessage;

extern void set_position(DispatchMessage* message, unsigned position);
extern void trace_dispatch(unsigned level, const char* format, ...);
extern unsigned dispatch_1(DispatchMessage* message);
extern unsigned dispatch_2(DispatchMessage* message);
extern unsigned dispatch_3(DispatchMessage* message);
extern unsigned dispatch_7(DispatchMessage* message);
extern unsigned dispatch_4(DispatchMessage* message);
extern unsigned dispatch_5(DispatchMessage* message);
extern unsigned dispatch_16(DispatchMessage* message);
extern unsigned dispatch_17(DispatchMessage* message);
extern unsigned dispatch_18(DispatchMessage* message);
extern unsigned dispatch_19(DispatchMessage* message);
extern unsigned dispatch_24(DispatchMessage* message);
extern unsigned dispatch_25(DispatchMessage* message);
extern unsigned dispatch_26(DispatchMessage* message);
extern unsigned dispatch_23(DispatchMessage* message);

int initialize_dispatcher(void) {
    return 0;
}

int dispatch_message(DispatchMessage* message) {
    unsigned result;
    result = 1280;
    set_position(message, 0);
    trace_dispatch(1, "Dispatch command 0x%08x\n", message->bytes[20]);
    switch (message->bytes[20]) {
    case 1: result = dispatch_1(message); break;
    case 2: result = dispatch_2(message); break;
    case 3: result = dispatch_3(message); break;
    case 7: result = dispatch_7(message); break;
    case 4: result = dispatch_4(message); break;
    case 5: result = dispatch_5(message); break;
    case 16: result = dispatch_16(message); break;
    case 17: result = dispatch_17(message); break;
    case 18: result = dispatch_18(message); break;
    case 19: result = dispatch_19(message); break;
    case 24: result = dispatch_24(message); break;
    case 25: result = dispatch_25(message); break;
    case 26: result = dispatch_26(message); break;
    case 23: result = dispatch_23(message); break;
    }
    trace_dispatch(1, "Dispatch complete err = %ld\n", result);
    return result;
}
