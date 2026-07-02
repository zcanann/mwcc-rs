/* mwcc's symbol table interleaves DEFINED data with function symbols by
 * source position: map_before, pick, map_after — the function symbol sits
 * between the two data symbols (static functions' LOCAL symbols precede the
 * data run and do not shift the slots). A whole-body block { { ... } } is
 * transparent. A narrow UNSIGNED parameter used as a byte-array index
 * re-extends like the explicit cast (clrlwi before the lbzx). */
typedef unsigned char u8;

u8 map_before[256] = { 1, 2, 3, 4 };

int pick(u8 c)
{
	{
		return map_before[c] & 0x40;
	}
}

u8 map_after[256] = { 5, 6, 7, 8 };

int pick_after(int c)
{
	return map_after[(u8)c];
}
