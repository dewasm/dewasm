# requires: memory/iwlo, rt/f32_from_bits
# The bit path preserves a NaN's sign and payload.
def fwlo(a, off) = Rt.f32_from_bits(iwlo(a, off))
