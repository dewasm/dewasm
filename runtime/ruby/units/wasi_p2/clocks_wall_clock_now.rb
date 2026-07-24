def p2_clocks_wall_clock_now
  t = Process.clock_gettime(Process::CLOCK_REALTIME, :nanosecond)
  { "seconds" => t / 1_000_000_000, "nanoseconds" => t % 1_000_000_000 }
end
