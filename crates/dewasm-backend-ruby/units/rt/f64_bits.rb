def f64_bits(x)
  [x].pack("E").unpack1("Q<")
end
