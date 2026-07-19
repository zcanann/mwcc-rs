// flags: -O4,p -inline auto,deferred

void deferred_compiled_first(void) {
}

asm void deferred_immediate_asm(void) {
    nofralloc
    blr
}

void deferred_compiled_last(void) {
}
