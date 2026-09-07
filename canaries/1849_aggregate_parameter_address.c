// TEV-driven aggregate, select, and shift boundary coverage.
// flags: -Cpp_exceptions off -pragma "cats off"
typedef struct Color { unsigned char r, g, b, a; } Color;
unsigned word(Color color) { return *(unsigned*)&color; }
unsigned channel(Color color) { unsigned rgba; rgba = *(unsigned*)&color; return color.r + color.a; }
unsigned pointer(Color *color) { Color **p = &color; return (*p)->r; }
unsigned after(unsigned color) { return *(unsigned*)&color; }
