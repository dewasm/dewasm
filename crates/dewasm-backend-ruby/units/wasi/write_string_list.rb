# requires: memory/iws, memory/iwsb, memory/init
def write_string_list(strings, list_ptr, buf_ptr)
  strings.each_with_index do |s, i|
    @memory.iws(list_ptr + i * 4, buf_ptr)
    @memory.init(buf_ptr, s, 0, s.bytesize)
    @memory.iwsb(buf_ptr + s.bytesize, 0)
    buf_ptr += s.bytesize + 1
  end
  ERRNO_SUCCESS
end
