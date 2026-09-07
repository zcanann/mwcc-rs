// Hardware address caches must follow the branch control-flow edges.
// flags: -Cpp_exceptions off -pragma "cats off"
typedef union { unsigned char byte; unsigned word; } Fifo;
volatile Fifo A : 0xCC008000;
volatile Fifo B : 0xCC018000;
void shared(unsigned c, unsigned v) {
 if(c) { A.byte=8; A.word=v; } else { A.byte=16; A.word=v+1; }
 A.word=v+2;
}
void different(unsigned c, unsigned v) {
 if(c) { A.word=v; } else { B.word=v+1; }
 A.word=v+2;
}
void nested(unsigned c, unsigned v) {
 if(c&1) { if(c&2) { A.word=v; } else { B.word=v+1; } }
 else { A.byte=16; A.word=v+2; }
 A.word=v+3;
}
