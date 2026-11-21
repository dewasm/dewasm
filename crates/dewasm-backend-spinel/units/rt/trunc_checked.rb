# requires: rt/trap
# `below` and `above` are the nearest floats outside the integer range, one on each side.
# The range is tested on the float: a truncation outside 64 bits has no word to land in.
def trunc_checked(x, below, above)
  trap("invalid conversion to integer") if x.nan?
  trap("integer overflow") if x <= below || x >= above
  x.truncate
end
