# requires: rt/trap
def i64_rem_u(a, b)
  trap("integer divide by zero") if b == 0
  a % b
end
