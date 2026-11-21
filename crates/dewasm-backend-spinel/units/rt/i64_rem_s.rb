# requires: rt/trap, rt/i64_div_s
def i64_rem_s(a, b)
  trap("integer divide by zero") if b == 0
  return 0 if b == -1
  a - i64_div_s(a, b) * b
end
