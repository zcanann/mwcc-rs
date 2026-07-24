// builds: GC/1.1 GC/1.1p1 GC/1.2.5 GC/1.2.5n

struct Vector {
	void set(const float&, const float&, const float&);
};

struct Object {
	Object()
	{
		first = 0;
		second = 0;
		third = 0;
		value.set(0.0f, 0.0f, 0.0f);
	}
	Vector value;
	void* first;
	void* second;
	void* third;
};

float probe() { return 1.25f; }
