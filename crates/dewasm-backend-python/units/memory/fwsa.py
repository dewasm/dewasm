# requires: memory/iwsa, rt/f32_bits
# Packing "<f" is bit-exact for every non-NaN value.
# A NaN takes the bit path because the double-to-float conversion quietens it, and wasm's f32.store is bit-preserving.
def fwsa(self, a, b, v):
    if v != v:
        self.iwsa(a, b, Rt.f32_bits(v))
        return
    a = (a + b) & 0xFFFFFFFF
    self.check(a, 4)
    self.data[a:a + 4] = struct.pack("<f", v)
