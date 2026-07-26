# requires: rt/f64_bits, rt/f64_from_bits
# Quiet a NaN (set the quiet bit), preserving sign and payload.
@staticmethod
def quiet_nan(x):
    return Rt.f64_from_bits(Rt.f64_bits(x) | 0x0008000000000000)
