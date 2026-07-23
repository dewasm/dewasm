# requires: rt/quiet_nan
def f64_promote(x)
  x.nan? ? quiet_nan(x) : x
end
