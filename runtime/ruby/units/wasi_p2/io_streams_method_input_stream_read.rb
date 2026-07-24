# Blocking reads stand in for the non-blocking variant: a synchronous
# host is always "ready", and the guest's poll loop terminates either way.
def p2_io_streams_method_input_stream_read(h, len)
  [:ok, res(h).io.readpartial(len)]
rescue EOFError
  [:err, [:closed, nil]]
rescue SystemCallError
  [:err, [:closed, nil]]
end
