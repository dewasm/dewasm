def p2_clocks_monotonic_clock_now
  Process.clock_gettime(Process::CLOCK_MONOTONIC, :nanosecond)
end
