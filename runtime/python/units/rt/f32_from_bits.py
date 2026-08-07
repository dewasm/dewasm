# requires: rt/f64_from_bits
# struct's float<->double conversion canonicalizes NaNs, losing sign and
# payload. Take a software path for NaNs (mirrors the Ruby backend).
@staticmethod
def f32_from_bits(b):
    if (b & 0x7F800000) == 0x7F800000 and (b & 0x7FFFFF) != 0:
        return Rt.f64_from_bits(((b >> 31) << 63) | 0x7FF0000000000000 | ((b & 0x7FFFFF) << 29))
    return struct.unpack("<f", struct.pack("<L", b))[0]
