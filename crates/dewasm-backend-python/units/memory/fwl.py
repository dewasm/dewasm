# requires: rt/f32_from_bits
# Unpacking "<f" is bit-exact for every non-NaN value.
# A NaN takes the bit path because the float-to-double conversion quietens it, and wasm's f32.load is bit-preserving.
def fwl(self, a):
    a &= 0xFFFFFFFF
    self.check(a, 4)
    r = struct.unpack("<f", self.data[a:a + 4])[0]
    if r != r:
        return Rt.f32_from_bits(int.from_bytes(self.data[a:a + 4], "little"))
    return r
