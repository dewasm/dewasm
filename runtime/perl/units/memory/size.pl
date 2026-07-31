sub size {
    return int(length($_[0]->{data}) / 65536);
}
