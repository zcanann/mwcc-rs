// A signed `char` global promotes to int with a trailing extsb (lbz zero-extends
// the byte, so it must be re-signed). On build 53 (unsigned char) the extsb is
// absent — the oracle checks both, since each runs against its own mwcceppc.
extern char g;
int gcharread(void){ return g; }
