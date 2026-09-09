volatile struct RegisterBank { unsigned value; } registers;
__declspec(weak) struct WeakBank { unsigned value; } weak_bank;
unsigned volatile_read(void) { return registers.value; }
void repeated_write(unsigned value) { registers.value = value; registers.value = value; }
unsigned weak_read(void) { return weak_bank.value; }
