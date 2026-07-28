# requires: mem/fill, mem/i32_store8, mem/i64_store
# wasi_pack_filestat <p> <buf_ptr> <filetype> <size>: writes the 64-byte WASI
# filestat struct into guest memory (ADR-34 D6, mirroring
# runtime/ruby/units/wasi/pack_filestat.rb's layout). dev/ino and
# atim/mtim/ctim are zeroed — Bash has no builtin to read real device/inode
# numbers or timestamps, a documented deviation from Ruby (which packs
# `File::Stat`'s real values). Layout: dev u64@0, ino u64@8, filetype u8@16
# (+7 pad), nlink u64@24 = 1, size u64@32, atim/mtim/ctim u64@40/48/56.
wasi_pack_filestat() {
  local __p=$1 __buf=$2 __filetype=$3 __size=$4
  mem_fill "$__p" "$__buf" 0 64 || return $?
  mem_i32_store8 "$__p" $(( __buf + 16 )) "$__filetype" || return $?
  mem_i64_store "$__p" $(( __buf + 24 )) 1 || return $?
  mem_i64_store "$__p" $(( __buf + 32 )) "$__size" || return $?
  R0=0
  return 0
}
