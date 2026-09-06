// flags: -char unsigned
/* The configured character mode is recorded even when no character type is used. */
int configured_character_mode(void) {
    return 7;
}
