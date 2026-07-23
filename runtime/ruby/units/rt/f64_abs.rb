# requires: rt/f64_bits, rt/f64_from_bits
def f64_abs(x) = f64_from_bits(f64_bits(x) & 0x7fff_ffff_ffff_ffff)
