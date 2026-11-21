# `below` and `above` are the nearest floats outside the integer range, one on each side.
def trunc_sat(x, below, above, min, max)
  return 0 if x.nan?
  return min if x <= below
  return max if x >= above
  x.truncate
end
