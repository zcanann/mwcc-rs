/* mwcc -inline auto INLINES a defined-inline function at its call sites. A
 * SINGLE-RETURN inline body substitutes with pure (variable/literal)
 * arguments; the @N counter still advances by the dropped out-of-line
 * body's labels. */
typedef unsigned char u8;
extern u8 lower_map[256];

inline int table_low(int c)
{
	return (int)lower_map[(u8)c];
}

inline int add_three(int a)
{
	return a + 3;
}

int f(int c)
{
	return table_low(c);
}

int g(int b)
{
	return add_three(b) * 2;
}
