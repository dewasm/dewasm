sub i64_clz {
    return 64 if $_[0] == 0;
    return 64 - length(sprintf('%b', $_[0]));
}
