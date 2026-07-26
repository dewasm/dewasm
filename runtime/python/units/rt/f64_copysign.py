# requires: rt/f64_bits, rt/f64_from_bits
@staticmethod
def f64_copysign(a, b):
    return Rt.f64_from_bits((Rt.f64_bits(a) & 0x7FFFFFFFFFFFFFFF) | (Rt.f64_bits(b) & 0x8000000000000000))
