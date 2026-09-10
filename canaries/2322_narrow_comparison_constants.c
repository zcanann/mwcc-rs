void half_equal(unsigned short x,int*p){ if(x==65535) *p=1; }
void half_less(unsigned short x,int*p){ if(x<65535) *p=1; }
void half_cast(unsigned short x,int*p){ if(x==(unsigned short)-1) *p=1; }
void word_equal(unsigned x,int*p){ if(x==65535) *p=1; }
void word_less(unsigned x,int*p){ if(x<65535) *p=1; }
void word_negative(unsigned x,int*p){ if(x==-1) *p=1; }
void word_negative_less(unsigned x,int*p){ if(x<-1) *p=1; }
void half_negative(unsigned short x,int*p){ if(x==-1) *p=1; }
void half_negative_less(unsigned short x,int*p){ if(x<-1) *p=1; }
void signed_large(short x,int*p){ if(x==65535) *p=1; }
void signed_limit(short x,int*p){ if(x<65535) *p=1; }
void signed_negative(short x,int*p){ if(x<-32769) *p=1; }
void half_le(unsigned short x,int*p){ if(x<=32768) *p=1; }
void half_gt(unsigned short x,int*p){ if(x>65535) *p=1; }
void half_ge(unsigned short x,int*p){ if(x>=65535) *p=1; }
void half_ne(unsigned short x,int*p){ if(x!=65535) *p=1; }
void half_over(unsigned short x,int*p){ if(x<65536) *p=1; }
void half_unsigned_negative(unsigned short x,int*p){ if(x<(unsigned)-1) *p=1; }
void half_left(unsigned short x,int*p){ if(65535>x) *p=1; }
void byte_equal(unsigned char x,int*p){ if(x==65535) *p=1; }
void byte_negative(unsigned char x,int*p){ if(x==-1) *p=1; }
void byte_below(unsigned char x,int*p){ if(x<-1) *p=1; }
void byte_over(unsigned char x,int*p){ if(x<65536) *p=1; }
void char_below(signed char x,int*p){ if(x<-32769) *p=1; }
void char_equal(signed char x,int*p){ if(x==65535) *p=1; }
void char_left(signed char x,int*p){ if(65535>x) *p=1; }
void half_load(unsigned short*q,int*p){ if(*q==65535) *p=1; }
void half_load_negative(unsigned short*q,int*p){ if(*q<-1) *p=1; }
void char_load(signed char*q,int*p){ if(*q==65535) *p=1; }
void char_load_limit(signed char*q,int*p){ if(*q<-32769) *p=1; }
struct Record { unsigned short count; short delta; signed char flag; };
void member_limit(struct Record*q,int*p){ if(q->count==65535) *p=1; }
void member_negative(struct Record*q,int*p){ if(q->count<-1) *p=1; }
void member_signed(struct Record*q,int*p){ if(q->delta<65535) *p=1; }
void member_byte(struct Record*q,int*p){ if(q->flag==65535) *p=1; }
