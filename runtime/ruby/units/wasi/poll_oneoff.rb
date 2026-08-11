# requires: memory/fill, memory/i32_load, memory/i32_load8_u, memory/i32_load16_u, memory/i64_load, memory/i32_store, memory/i32_store8, memory/i32_store16, memory/i64_store
# poll_oneoff waits until at least one subscription is ready, then writes one event per ready subscription (WASI p1 layout: 48-byte subscriptions in,
# 32-byte events out).
# Only fd_read on stdin actually blocks (via IO.select);
# regular files, stdout/stderr, and every fd_write are treated as immediately ready, and unknown fds report EBADF.
# Clock subscriptions set the wait deadline; if it elapses with no fd ready, the due clock subs fire.
# Motivated by event-loop guests such as the QuickJS REPL, which blocks here on stdin between prompts.
def wasi_poll_oneoff(in_ptr, out_ptr, nsubs, nevents_ptr)
  return ERRNO_INVAL if nsubs == 0
  ready = []   # [userdata, error, type, nbytes, flags] resolvable without waiting
  waiters = [] # [userdata, type, io] fd_read on stdin: needs a host wait
  clocks = []  # [userdata, rel_ns]
  nsubs.times do |i|
    base = in_ptr + i * 48
    userdata = @memory.i64_load(base)
    tag = @memory.i32_load8_u(base + 8)
    case tag
    when 0 # clock
      clock_id = @memory.i32_load(base + 16)
      timeout = @memory.i64_load(base + 24)
      flags = @memory.i32_load16_u(base + 40)
      now =
        if clock_id == 0
          Process.clock_gettime(Process::CLOCK_REALTIME, :nanosecond)
        else
          Process.clock_gettime(Process::CLOCK_MONOTONIC, :nanosecond)
        end
      rel = (flags & 1) != 0 ? [timeout - now, 0].max : timeout
      clocks << [userdata, rel]
    when 1, 2 # fd_read / fd_write
      fd = @memory.i32_load(base + 16)
      io = @fds[fd]
      if io.nil? || io.is_a?(WasiDir)
        ready << [userdata, ERRNO_BADF, tag, 0, 0]
      elsif tag == 1 && @std_ios[0].equal?(io)
        waiters << [userdata, tag, io]
      else
        nbytes = 1
        if tag == 1 && io.respond_to?(:stat)
          nbytes = (io.stat.size - io.pos rescue 1)
        end
        ready << [userdata, 0, tag, nbytes, 0]
      end
    else
      return ERRNO_INVAL
    end
  end

  events = ready
  if events.empty?
    if !waiters.empty?
      timeout_s = clocks.empty? ? nil : clocks.map { |c| c[1] }.min / 1e9
      readable, = IO.select(waiters.map { |w| w[2] }, nil, nil, timeout_s)
      if readable
        waiters.each do |userdata, type, io|
          events << [userdata, 0, type, 1, 0] if readable.include?(io)
        end
      end
    elsif !clocks.empty?
      sleep(clocks.map { |c| c[1] }.min / 1e9)
    end
    if events.empty? && !clocks.empty?
      due = clocks.map { |c| c[1] }.min
      clocks.each { |userdata, rel| events << [userdata, 0, 0, 0, 0] if rel <= due }
    end
  end

  events.each_with_index do |(userdata, error, type, nbytes, flags), i|
    ev = out_ptr + i * 32
    @memory.fill(ev, 0, 32)
    @memory.i64_store(ev, userdata)
    @memory.i32_store16(ev + 8, error)
    @memory.i32_store8(ev + 10, type)
    @memory.i64_store(ev + 16, nbytes)
    @memory.i32_store16(ev + 24, flags)
  end
  @memory.i32_store(nevents_ptr, events.size)
  ERRNO_SUCCESS
end
