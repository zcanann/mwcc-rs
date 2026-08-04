/* Two parameters survive one call. The allocator assigns f31 to the later
   parameter and f30 to the earlier one; GC/1.3--2.7 issue the independent
   entry copies from the lower saved home first. */
extern float transform(float value);

float retain_float_pair(float value, float left, float right)
{
	float transformed = transform(value);
	return transformed + left * right;
}
