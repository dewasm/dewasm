def p2_io_streams_method_output_stream_write(h, bytes)
  res(h).io.write(bytes)
  [:ok, nil]
rescue SystemCallError
  [:err, [:closed, nil]]
end
