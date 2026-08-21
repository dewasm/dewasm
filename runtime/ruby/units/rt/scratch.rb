# The 8-byte staging area the float bit conversions write a value into and read back under another type.
# A store and its read-back are two separate operations, so the buffer must never be reachable from two threads at once: it hangs off the receiver, which is the artifact instance for a generated body and the memory instance for a memory unit, both of which already cannot be shared across threads (their linear memory is not either).
# Call sites read `@scratch` inline and only fall back here to create it.
def scratch
  @scratch ||= begin
    saved = Warning[:experimental]
    begin
      Warning[:experimental] = false
      IO::Buffer.new(8)
    ensure
      Warning[:experimental] = saved
    end
  end
end
