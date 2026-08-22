def wasi_filetype(stat)
  if stat.directory?
    3
  elsif stat.chardev?
    2
  elsif stat.blockdev?
    1
  elsif stat.symlink?
    7
  elsif stat.socket?
    6
  elsif stat.file?
    4
  else
    0
  end
end
private :wasi_filetype
