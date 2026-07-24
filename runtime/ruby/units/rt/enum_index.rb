# requires: rt/trap
# The discriminant of a host enum value (its case symbol, ADR-20).
def enum_index(cases, value)
  i = cases.index(value)
  trap("invalid enum case #{value.inspect}") if i.nil?
  i
end
