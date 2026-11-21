# requires: rt/f64_bits, rt/f64_from_bits
def f64_neg(x) = f64_from_bits(f64_bits(x) ^ I64_MIN)
