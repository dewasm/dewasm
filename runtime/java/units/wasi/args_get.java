// requires: wasi/write_string_list
int wasi_args_get(int argvPtr, int bufPtr) {
    return write_string_list(args, argvPtr, bufPtr);
}
