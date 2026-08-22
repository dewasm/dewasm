# requires: rt/quiet_nan
def fsqrt(x)
  return quiet_nan(x) if x.nan?
  return Float::NAN if x < 0.0
  return x if x == 0.0 # preserves -0.0
  Math.sqrt(x)
end
