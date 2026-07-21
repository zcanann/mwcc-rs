// The OSReset cancellation walk: `next` must survive the conditional call while
// the current node remains in r3. The preceding asm function preserves the
// legacy compiler's source-edge schedule used by the real SDK translation unit.
// builds: GC/1.2.5

asm void preceding_reset_asm(void)
{
    nofralloc
    blr
}

typedef struct ResetNode ResetNode;
struct ResetNode {
    unsigned char pad_to_state[712];
    unsigned short state;
    unsigned char pad_to_next[50];
    ResetNode* next;
};

extern ResetNode* reset_head;
extern void cancel_reset_node(ResetNode* node);

void walk_reset_nodes(void)
{
    ResetNode* node;
    ResetNode* next;

    for (node = reset_head; node; node = next) {
        next = node->next;
        switch (node->state) {
        case 1:
        case 4:
            cancel_reset_node(node);
            break;
        }
    }
}
