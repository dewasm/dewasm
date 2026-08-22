# requires: rt/f32, rt/f32_bits, rt/f32_from_bits
@staticmethod
def f32_demote(x):
    if x != x:
        return Rt.f32_from_bits((Rt.f32_bits(x) & 0x80000000) | 0x7FC00000)
    return Rt.f32(x)
