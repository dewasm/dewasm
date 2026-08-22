# requires: rt/f32_bits, rt/f32_from_bits
@staticmethod
def f32_abs(x):
    return Rt.f32_from_bits(Rt.f32_bits(x) & 0x7FFFFFFF)
