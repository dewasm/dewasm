def f64_from_bits(b)
  [b].pack("Q<").unpack1("E")
end
