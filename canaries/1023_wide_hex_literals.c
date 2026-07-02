/* Full-width hex literals are valid C constants whose bits are the value —
 * 0xFFFFFFFFU wraps to -1 in a signed-64 parse; suffixed forms drop their
 * hints. */
unsigned int g;

int all_ones(void)
{
	return 0xFFFFFFFFU;
}

void store_wide(void)
{
	g = 0xDEADBEEFU;
}
