# requires: rt/quiet_nan
def ftrunc(x)
  return quiet_nan(x) if x.nan?
  return x unless x.finite?
  return x if x == 0.0 # preserves -0.0
  r = x.truncate
  r == 0 && x < 0.0 ? -0.0 : r.to_f
end
