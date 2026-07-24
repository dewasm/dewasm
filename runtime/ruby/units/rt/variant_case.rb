# requires: rt/trap
# The case index of a host variant value `[case_symbol, payload]` (ADR-20).
def variant_case(cases, value)
  i = cases.index(value[0])
  trap("invalid variant case #{value[0].inspect}") if i.nil?
  i
end
