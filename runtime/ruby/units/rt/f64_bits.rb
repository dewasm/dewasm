# requires: rt/scratch
def f64_bits(x) = (s = @scratch || scratch; s.set_value(:f64, 0, x); s.get_value(:u64, 0))
