# requires: rt/trap
def i64_div_u(a, b)
  trap("integer divide by zero") if b == 0
  a / b
end
