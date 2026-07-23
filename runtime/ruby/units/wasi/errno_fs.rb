# Filesystem-only errno codes (ADR-14): kept out of the always-bundled
# wasi/_class prelude so a stdio-only WASI module (no path_* / fs-only
# fd_* imports) doesn't carry them.
ERRNO_ACCES = 2
ERRNO_EXIST = 20
ERRNO_ISDIR = 31
ERRNO_LOOP = 32
ERRNO_NAMETOOLONG = 37
ERRNO_NOENT = 44
ERRNO_NOTDIR = 54
ERRNO_NOTEMPTY = 55
ERRNO_PERM = 63
ERRNO_NOTCAPABLE = 76
