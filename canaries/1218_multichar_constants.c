/* Metrowerks packs multi-character tag constants in source order into a
 * 32-bit int. GameCube code uses these pervasively for parameter and asset IDs. */
int two_character_tag(void) { return 'AB'; }
int four_character_tag(void) { return 'fp00'; }
