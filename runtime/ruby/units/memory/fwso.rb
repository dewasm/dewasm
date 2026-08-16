# requires: memory/iwso, rt/f32_bits
def fwso(a, off, v) = iwso(a, off, Rt.f32_bits(v))
