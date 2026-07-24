# requires: rt/trap
# `call`'s checks, but returning the slot's tail-entry callable for the
# trampoline (ADR-18) instead of invoking: a slot's optional third element
# is the body method of a tail-calling function; entries without one
# complete in a single frame anyway.
def tail_ref(i, type_key)
  Rt.trap("undefined element") if i >= @slots.size
  slot = @slots[i]
  Rt.trap("uninitialized element") if slot.nil?
  ty, func, tail = slot
  Rt.trap("indirect call type mismatch") unless ty == type_key
  tail || func
end
