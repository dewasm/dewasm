# requires: rt/f64_bits
@staticmethod
def f32_bits(x):
    if x != x:
        b = Rt.f64_bits(x)
        payload = (b >> 29) & 0x7FFFFF
        if payload == 0:
            payload = 0x400000
        return (((b >> 63) & 1) << 31) | 0x7F800000 | payload
    return struct.unpack("<L", struct.pack("<f", x))[0]
