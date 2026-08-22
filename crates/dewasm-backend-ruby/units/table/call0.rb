# requires: rt/trap
# Same dispatch and trap contract as `call`, with no `*args`/splat array built on either side.
def call0(i, type_key)
  Rt.trap("undefined element") if i >= @slots.size
  slot = @slots[i]
  Rt.trap("uninitialized element") if slot.nil?
  ty, func = slot
  Rt.trap("indirect call type mismatch") unless ty == type_key
  func.call
end
