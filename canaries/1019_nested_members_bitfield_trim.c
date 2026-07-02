/* An EMBEDDED struct-value member folds into its base — p->state.eof is one
 * access at offset(state)+offset(eof), no intermediate load — and mwcc TRIMS
 * a bit-field container to the bytes its bits use (4 bits -> the next byte
 * member lands at +1; 9-12 bits -> +2; an int member still aligns to 4; the
 * container type's alignment governs the struct). */
typedef struct FileState {
	unsigned int io_state : 3;
	unsigned int free_buffer : 1;
	unsigned char eof;
	unsigned char error;
} FileState;

typedef struct File {
	int handle;
	int mode;
	FileState state;
} File;

void clear_state(File* p)
{
	p->state.eof = 0;
	p->state.error = 0;
}

typedef struct Trim12 {
	unsigned int bits : 12;
	unsigned char tail;
} Trim12;

int read_tail(Trim12* p)
{
	return p->tail;
}

typedef struct TrimInt {
	unsigned int bits : 4;
	int aligned_tail;
} TrimInt;

int read_aligned(TrimInt* p)
{
	return p->aligned_tail;
}

typedef struct WithNested {
	FileState s;
	unsigned char after;
} WithNested;

int read_after(WithNested* p)
{
	return p->after;
}
