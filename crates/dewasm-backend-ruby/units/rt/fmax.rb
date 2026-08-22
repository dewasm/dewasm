def fmax(a, b)
  return Float::NAN if a.nan? || b.nan?
  if a > b
    a
  elsif b > a
    b
  elsif a == 0.0 && b == 0.0
    (1.0 / a > 0 || 1.0 / b > 0) ? 0.0 : -0.0
  else
    a
  end
end
