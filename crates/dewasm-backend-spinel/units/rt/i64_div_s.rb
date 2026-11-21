# requires: rt/trap
# The host's `/` rounds down, and wasm rounds toward zero.
def i64_div_s(a, b)
  trap("integer divide by zero") if b == 0
  trap("integer overflow") if a == I64_MIN && b == -1
  q = a / b
  q += 1 if q < 0 && q * b != a
  q
end
