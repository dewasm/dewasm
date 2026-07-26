# requires: rt/f32_bits, rt/f32_from_bits
@staticmethod
def f32_copysign(a, b):
    return Rt.f32_from_bits((Rt.f32_bits(a) & 0x7FFFFFFF) | (Rt.f32_bits(b) & 0x80000000))
