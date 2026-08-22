# requires: rt/quiet_nan
def ffloor(x)
  return quiet_nan(x) if x.nan?
  return x unless x.finite?
  return x if x == 0.0 # preserves -0.0
  x.floor.to_f
end
