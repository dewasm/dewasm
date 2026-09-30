# Shared `fstflags` handling for `fd_filestat_set_times` and `path_filestat_set_times`.
# The `fstflags` bits are:
# ATIM = 1, ATIM_NOW = 2, MTIM = 4, MTIM_NOW = 8.
#
# Returns ERRNO_INVAL when a field and its *_NOW variant are both set, else `nil`.
# A value and "use the current time" contradict each other.
def validate_fstflags(fstflags)
  return ERRNO_INVAL if (fstflags & 0x1 != 0) && (fstflags & 0x2 != 0)
  return ERRNO_INVAL if (fstflags & 0x4 != 0) && (fstflags & 0x8 != 0)
  nil
end
private :validate_fstflags

# The `[atime, mtime]` to apply, each one of:
# - an explicit nanosecond value;
# - the current time (*_NOW);
# - the file's existing time when neither bit is set, so `utime` leaves that field untouched.
def resolve_times(stat, atim, mtim, fstflags)
  now = Time.now
  a = if fstflags & 0x1 != 0 then nanos_to_time(atim)
      elsif fstflags & 0x2 != 0 then now
      else stat.atime
      end
  m = if fstflags & 0x4 != 0 then nanos_to_time(mtim)
      elsif fstflags & 0x8 != 0 then now
      else stat.mtime
      end
  [a, m]
end
private :resolve_times

def nanos_to_time(nanos)
  Time.at(nanos / 1_000_000_000, nanos % 1_000_000_000, :nanosecond)
end
private :nanos_to_time
