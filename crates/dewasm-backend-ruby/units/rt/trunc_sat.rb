def trunc_sat(x, min, max)
  return 0 if x.nan?
  t = x.infinite? ? (x > 0 ? max : min) : x.truncate
  t.clamp(min, max)
end
