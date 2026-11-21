def f64_bits(x)
  [x].pack("E").unpack1("q<")
end
