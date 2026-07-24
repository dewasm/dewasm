def p2_filesystem_types_method_descriptor_read_via_stream(h, offset)
  io = File.open(res(h).host_path, "rb")
  io.seek(offset)
  [:ok, res_new(InStream.new(io, true))]
rescue SystemCallError
  [:err, :access]
end
