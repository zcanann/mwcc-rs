struct Record { unsigned char byte; unsigned short half; unsigned word; };
extern unsigned char byte_mask;
extern unsigned short half_mask;
extern unsigned word_mask;
extern int query(struct Record *);
int retained_byte(struct Record *p) {
 unsigned char bits; int result=query(p);
 if(result<0) { bits=(unsigned char)(p->byte & byte_mask); if(bits&32) return query(p); if(bits&64) return query(p); }
 return result;
}
int retained_half(struct Record *p) {
 unsigned short bits; int result=query(p);
 if(result<0) { bits=(unsigned short)(p->half & half_mask); if(bits&256) return query(p); if(bits&1024) return query(p); }
 return result;
}
int retained_word(struct Record *p) {
 unsigned bits; int result=query(p);
 if(result<0) { bits=p->word & word_mask; if(bits&65536) return query(p); if(bits&262144) return query(p); }
 return result;
}
int reversed_byte(struct Record *p) {
 unsigned char bits; int result=query(p);
 if(result<0) { bits=(unsigned char)(byte_mask & p->byte); if(bits&32) return query(p); if(bits&64) return query(p); }
 return result;
}
int after_byte(struct Record *p) {
 unsigned bits; int result=query(p);
 if(result<0) { bits=p->byte & byte_mask; query(p); return bits; }
 return result;
}
int after_half(struct Record *p) {
 unsigned bits; int result=query(p);
 if(result<0) { bits=p->half & half_mask; query(p); return bits; }
 return result;
}
int after_word(struct Record *p) {
 unsigned bits; int result=query(p);
 if(result<0) { bits=p->word & word_mask; query(p); return bits; }
 return result;
}
int after_subtract(struct Record *p) {
 unsigned bits; int result=query(p);
 if(result<0) { bits=word_mask-p->word; query(p); return bits; }
 return result;
}
