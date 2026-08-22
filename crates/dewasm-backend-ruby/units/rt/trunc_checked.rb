# requires: rt/trap
def trunc_checked(x, min, max)
  trap("invalid conversion to integer") if x.nan?
  trap("integer overflow") if x.infinite?
  t = x.truncate
  trap("integer overflow") if t < min || t > max
  t
end
