# requires: wasi/write_string_list
wasi_environ_get() {
  wasi_write_string_list "$1" "${1}wenv" "$2" "$3"
  return $?
}
