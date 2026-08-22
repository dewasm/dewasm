# requires: rt/f64_bits, rt/f64_from_bits
@staticmethod
def f64_abs(x):
    return Rt.f64_from_bits(Rt.f64_bits(x) & 0x7FFFFFFFFFFFFFFF)
