/* A real function's static local is a plain LOCAL object `name$K` — const>8B
 * -> .rodata, const<=8B -> .sdata2, non-const -> .sdata — accessed like a
 * global. The $K numbers ride the @N counter: statics lead their owner's
 * block (the first function's static is $4 against the base-5 counter) and
 * the owner's pool constants shift by the static count. */
typedef float f32;

int first(int i)
{
	static const int table[] = {10, 20, 30, 40};
	return table[i];
}

f32 coeff_pick(int i)
{
	static const f32 coeff[] = {1.5f, 2.5f, 3.5f};
	return coeff[i];
}

int scalar_counter(void)
{
	static int counter = 5;
	return counter;
}

int small_pair(int i)
{
	static const int two[] = {7, 8};
	return two[i];
}

double pooled(double x)
{
	return x * 3.0;
}

int after_pool(int i)
{
	static const int late[] = {9, 10, 11};
	return late[i];
}
