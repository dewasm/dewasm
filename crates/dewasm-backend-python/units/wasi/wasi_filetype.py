# Map a stat result's st_mode to a WASI filetype tag (S_IFMT bits, avoiding a dependency on the `stat` module).
def wasi_filetype(self, st):
    fmt = st.st_mode & 0o170000
    if fmt == 0o040000:
        return 3  # directory
    if fmt == 0o020000:
        return 2  # character device
    if fmt == 0o060000:
        return 1  # block device
    if fmt == 0o120000:
        return 7  # symbolic link
    if fmt == 0o140000:
        return 6  # socket (stream)
    if fmt == 0o100000:
        return 4  # regular file
    return 0  # unknown
