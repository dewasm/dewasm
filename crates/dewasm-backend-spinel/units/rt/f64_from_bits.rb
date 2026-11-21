def f64_from_bits(b)
  [b].pack("q<").unpack1("E")
end
