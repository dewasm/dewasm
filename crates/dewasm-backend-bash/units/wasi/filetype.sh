# wasi_filetype <path>: WASI filetype for <path> via the test builtins, in lstat order.
# It is the bash analogue of crates/dewasm-backend-ruby/units/wasi/wasi_filetype.rb.
# The Ruby helper checks a File::Stat/lstat result.
# `-h` (symlink) must come first, since `-d`/`-c`/`-b`/`-f` dereference a symlink.
# Testing them first would report a directory-symlink as a plain directory.
# A FIFO (`-p`) has no dedicated WASI filetype and reports unknown (0), matching Ruby.
# Ruby's helper has no `fifo?` branch either, so a named pipe falls through to its own `else` case.
# A socket (`-S`) is reported as socket_stream (6).
# That is the same single bucket Ruby's `socket?` branch uses.
# Always succeeds (a nonexistent path reports unknown/0); R1 is the filetype.
wasi_filetype() {
  local __path=$1
  if [[ -h $__path ]]; then
    R1=7 # symbolic_link
  elif [[ -d $__path ]]; then
    R1=3 # directory
  elif [[ -c $__path ]]; then
    R1=2 # character_device
  elif [[ -b $__path ]]; then
    R1=1 # block_device
  elif [[ -p $__path ]]; then
    R1=0 # fifo: no dedicated WASI filetype (matches Ruby's fallthrough)
  elif [[ -S $__path ]]; then
    R1=6 # socket_stream
  elif [[ -f $__path ]]; then
    R1=4 # regular_file
  else
    R1=0 # unknown
  fi
  R0=0
  return 0
}
