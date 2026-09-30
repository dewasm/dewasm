# requires: rt/trap
# `call`'s checks, raised at the same point in execution order.
# It returns the slot's callable for the trampoline instead of invoking it.
# A slot's optional third element is the body method of a tail-calling function.
# A slot without one completes in a single frame anyway.
def tail_ref(i, type_key)
  Rt.trap("undefined element") if i >= @slots.size
  slot = @slots[i]
  Rt.trap("uninitialized element") if slot.nil?
  ty, func, tail = slot
  Rt.trap("indirect call type mismatch") unless ty == type_key
  tail || func
end
