# requires: memory/i32_store, memory/i32_store8, memory/init
def write_string_list(strings, list_ptr, buf_ptr)
  strings.each_with_index do |s, i|
    @memory.i32_store(list_ptr + i * 4, buf_ptr)
    @memory.init(buf_ptr, s, 0, s.bytesize)
    @memory.i32_store8(buf_ptr + s.bytesize, 0)
    buf_ptr += s.bytesize + 1
  end
  ERRNO_SUCCESS
end
