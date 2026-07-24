def p2_io_streams_method_output_stream_blocking_write_and_flush(h, bytes)
  io = res(h).io
  io.write(bytes)
  io.flush
  [:ok, nil]
rescue SystemCallError
  [:err, [:closed, nil]]
end
