# requires: memory/iwla, rt/f32_from_bits
# The bit path preserves a NaN's sign and payload.
def fwla(a, b) = f32_from_bits(iwla(a, b))
