# requires: rt/trap, rt/i64_lt_u
def i64_div_u(a, b)
  trap("integer divide by zero") if b == 0
  # A divisor of 2**63 or more goes into the dividend at most once.
  return i64_lt_u(a, b) ? 0 : 1 if b < 0
  return a / b if a >= 0
  # The dividend is 2**63 or more: divide its half, then correct the last bit of the quotient.
  q = (((a >> 1) & 0x7fff_ffff_ffff_ffff) / b) << 1
  q += 1 unless i64_lt_u(a - q * b, b)
  q
end
