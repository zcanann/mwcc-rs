typedef struct FloatBox {
    float value;
} FloatBox;

float nested_located_fused_triplets(FloatBox* a, FloatBox* b, FloatBox* c,
                                    FloatBox* d, FloatBox* e, FloatBox* f)
{
    return (a->value * b->value + c->value) -
           (d->value * e->value + f->value);
}
