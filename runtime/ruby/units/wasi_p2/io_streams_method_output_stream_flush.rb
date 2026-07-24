def p2_io_streams_method_output_stream_flush(h)
  res(h).io.flush
  [:ok, nil]
rescue SystemCallError
  [:err, [:closed, nil]]
end
