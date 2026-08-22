# requires: wasi/write_string_list
wasi_args_get() {
  wasi_write_string_list "$1" "${1}wargs" "$2" "$3"
  return $?
}
