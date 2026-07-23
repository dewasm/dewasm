# requires: memory/init
require "securerandom"

def wasi_random_get(buf_ptr, len)
  @memory.init(buf_ptr, SecureRandom.bytes(len), 0, len)
  ERRNO_SUCCESS
end
