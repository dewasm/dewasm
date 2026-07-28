# Validate the fstflags and compute the (atime_ns, mtime_ns) tuple for
# os.utime, filling any field not being set from the current stat `st`
# (ADR-40). ATIM with ATIM_NOW (or MTIM with MTIM_NOW) is a contradiction and
# yields INVAL; returns (times, err).
def fst_times(self, st, atim, mtim, fst_flags):
    if (fst_flags & 0x1 and fst_flags & 0x2) or (fst_flags & 0x4 and fst_flags & 0x8):
        return (None, self.ERRNO_INVAL)
    now = time.time_ns()
    if fst_flags & 0x1:  # ATIM
        a = atim
    elif fst_flags & 0x2:  # ATIM_NOW
        a = now
    else:
        a = st.st_atime_ns
    if fst_flags & 0x4:  # MTIM
        m = mtim
    elif fst_flags & 0x8:  # MTIM_NOW
        m = now
    else:
        m = st.st_mtime_ns
    return ((a, m), None)
