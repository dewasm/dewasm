# requires: rt/trap
# Bounds check for an (active) element segment before initializing it;
# also catches empty segments whose offset is past the end.
def check_range(offset, count)
  Rt.trap("out of bounds table access") if offset + count > @funcs.size
end
