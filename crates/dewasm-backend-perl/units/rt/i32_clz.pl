sub i32_clz {
    return 32 if $_[0] == 0;
    return 32 - length(sprintf('%b', $_[0]));
}
