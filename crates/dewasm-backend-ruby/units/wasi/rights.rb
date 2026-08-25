# WASI p1 rights bits and the per-filetype masks used to grant and enforce capabilities.
# Kept in its own unit so every rights-aware syscall can require it without pulling extra filesystem code.
# The `@fd_meta` map
# (fd => [base, inheriting, fdflags, filetype]) that these operate on is seeded in wasi/_class.
RIGHT_FD_DATASYNC = 1 << 0
RIGHT_FD_READ = 1 << 1
RIGHT_FD_SEEK = 1 << 2
RIGHT_FD_FDSTAT_SET_FLAGS = 1 << 3
RIGHT_FD_SYNC = 1 << 4
RIGHT_FD_TELL = 1 << 5
RIGHT_FD_WRITE = 1 << 6
RIGHT_FD_ADVISE = 1 << 7
RIGHT_FD_ALLOCATE = 1 << 8
RIGHT_PATH_CREATE_DIRECTORY = 1 << 9
RIGHT_PATH_CREATE_FILE = 1 << 10
RIGHT_PATH_LINK_SOURCE = 1 << 11
RIGHT_PATH_LINK_TARGET = 1 << 12
RIGHT_PATH_OPEN = 1 << 13
RIGHT_FD_READDIR = 1 << 14
RIGHT_PATH_READLINK = 1 << 15
RIGHT_PATH_RENAME_SOURCE = 1 << 16
RIGHT_PATH_RENAME_TARGET = 1 << 17
RIGHT_PATH_FILESTAT_GET = 1 << 18
RIGHT_PATH_FILESTAT_SET_SIZE = 1 << 19
RIGHT_PATH_FILESTAT_SET_TIMES = 1 << 20
RIGHT_FD_FILESTAT_GET = 1 << 21
RIGHT_FD_FILESTAT_SET_SIZE = 1 << 22
RIGHT_FD_FILESTAT_SET_TIMES = 1 << 23
RIGHT_PATH_SYMLINK = 1 << 24
RIGHT_PATH_REMOVE_DIRECTORY = 1 << 25
RIGHT_PATH_UNLINK_FILE = 1 << 26
RIGHT_POLL_FD_READWRITE = 1 << 27

# Rights a regular-file fd may carry, and the two directory masks.
# A directory fd's base is narrowed to DIR_BASE_RIGHTS (so a requested FD_SEEK or FD_WRITE is dropped rather than granted), and its inheriting rights add the file rights it may pass to children (DIR_INHERITING_RIGHTS).
# These mirror the fixed masks wasi-common applies at path_open.
FILE_BASE_RIGHTS = RIGHT_FD_DATASYNC | RIGHT_FD_READ | RIGHT_FD_SEEK |
                   RIGHT_FD_FDSTAT_SET_FLAGS | RIGHT_FD_SYNC | RIGHT_FD_TELL |
                   RIGHT_FD_WRITE | RIGHT_FD_ADVISE | RIGHT_FD_ALLOCATE |
                   RIGHT_FD_FILESTAT_GET | RIGHT_FD_FILESTAT_SET_SIZE |
                   RIGHT_FD_FILESTAT_SET_TIMES | RIGHT_POLL_FD_READWRITE
DIR_BASE_RIGHTS = RIGHT_PATH_CREATE_DIRECTORY | RIGHT_PATH_CREATE_FILE |
                  RIGHT_PATH_LINK_SOURCE | RIGHT_PATH_LINK_TARGET |
                  RIGHT_PATH_OPEN | RIGHT_FD_READDIR | RIGHT_PATH_READLINK |
                  RIGHT_PATH_RENAME_SOURCE | RIGHT_PATH_RENAME_TARGET |
                  RIGHT_PATH_FILESTAT_GET | RIGHT_PATH_FILESTAT_SET_SIZE |
                  RIGHT_PATH_FILESTAT_SET_TIMES | RIGHT_FD_FILESTAT_GET |
                  RIGHT_FD_FILESTAT_SET_TIMES | RIGHT_PATH_SYMLINK |
                  RIGHT_PATH_REMOVE_DIRECTORY | RIGHT_PATH_UNLINK_FILE
DIR_INHERITING_RIGHTS = DIR_BASE_RIGHTS | FILE_BASE_RIGHTS

# True when fd's stored base rights include `right`.
# An fd with no meta
# (should not happen: every entry is seeded) is treated as fully capable.
def fd_has_right?(fd, right)
  meta = @fd_meta[fd]
  meta.nil? || (meta[0] & right) == right
end
private :fd_has_right?
