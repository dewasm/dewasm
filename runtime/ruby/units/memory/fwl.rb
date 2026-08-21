# requires: memory/iwl, rt/f32_from_bits
# The bit path preserves a NaN's sign and payload.
def fwl(a) = f32_from_bits(iwl(a))
