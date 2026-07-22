// C++ at O3 with deferred inlining keeps the canonical saved-pointer-before-LR
// epilogue for a call-result store through that pointer. The Strikers runtime
// configuration exercises the O4 sibling, where the reloads reverse.
// flags: -O3 -inline deferred
extern "C" int produced(void);

extern "C" void store_call_result_o3(int *destination) { *destination = produced(); }
