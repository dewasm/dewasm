def p2_random_random_get_random_bytes(len)
  File.binread("/dev/urandom", len)
end
