# requires: rt/scratch
def f64_from_bits(b) = (s = @scratch || scratch; s.set_value(:u64, 0, b); s.get_value(:f64, 0))
