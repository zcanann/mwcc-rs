// C linkage does not remove the C++ frontend first-use guard.
// flags: -Cpp_exceptions off -pragma "cats off"
extern "C" {
char* text() { static char* p="guarded"; return p; }
}
