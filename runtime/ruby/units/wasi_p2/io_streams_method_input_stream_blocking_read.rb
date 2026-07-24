def p2_io_streams_method_input_stream_blocking_read(h, len)
  [:ok, res(h).io.readpartial(len)]
rescue EOFError
  [:err, [:closed, nil]]
rescue SystemCallError
  [:err, [:closed, nil]]
end
