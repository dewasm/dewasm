# requires: mem/fill, mem/i32_load, mem/i32_load8_u, mem/i32_load16_u, mem/i64_load, mem/i32_store, mem/i32_store8, mem/i32_store16, mem/i64_store
# poll_oneoff waits until at least one subscription is ready, then writes one
# event per ready subscription (WASI p1: 48-byte subscriptions in, 32-byte
# events out; mirrors the Ruby unit, ADR-34 D4). fd_write on any writable fd, a
# regular-file fd_read, and stdout/stderr reads are immediately ready; an
# unknown or directory fd reports EBADF; only fd_read on stdin blocks. A clock
# subscription sets the wait deadline; if it elapses with no fd ready the
# soonest clock(s) fire, exactly as Ruby (ready fd events win outright, so a
# clock is never consulted once any fd is ready). stdin blocks via `read -t
# <deadline>` and the bytes that arrive are held in the pushback buffer
# (<p>wpush, a space-separated byte-ordinal list shared with fd_read); a
# non-tty stdin waits for one byte with `read -d '' -n 1`, while a tty stdin
# waits for a whole canonical line with a plain `read` (`-n 1` toggles ICANON
# per byte and each restore makes the pty line discipline re-echo the pending
# line — see fd_read); a clock-only wait sleeps with a bash-only
# coproc timer (a process substitution opened `<>` is rejected on some hosts, so
# a coproc that blocks on its own pipe is the portable sleep). `now` comes from
# EPOCHREALTIME, with monotonic falling back to realtime (the ADR-12 clock
# deviation). LC_ALL=C keeps the byte ordinal conversion byte-granular.
wasi_poll_oneoff() {
  local __p=$1 __in=$2 __out=$3 __nsubs=$4 __nevents_ptr=$5
  if (( __nsubs == 0 )); then
    R0=28 # EINVAL
    return 0
  fi
  local -n __fds=${__p}wfds
  local -n __tell=${__p}wtell
  local -n __push=${__p}wpush
  local LC_ALL=C
  local __i __base __tag __fd __kind __ud __cid __timeout __flags
  local __now __s __us __rel __avail
  local -a __ev=()            # ready events, each "userdata errno type nbytes flags"
  local -a __wait=()          # userdata of stdin fd_read waiters
  local -a __cud=() __crel=() # clock userdata / relative-ns deadline (>=0)
  for (( __i = 0; __i < __nsubs; __i++ )); do
    __base=$(( __in + __i * 48 ))
    mem_i64_load "$__p" "$__base" || return $?
    __ud=$R0
    mem_i32_load8_u "$__p" $(( __base + 8 )) || return $?
    __tag=$R0
    if (( __tag == 0 )); then
      mem_i32_load "$__p" $(( __base + 16 )) || return $?
      __cid=$R0
      mem_i64_load "$__p" $(( __base + 24 )) || return $?
      __timeout=$R0
      mem_i32_load16_u "$__p" $(( __base + 40 )) || return $?
      __flags=$R0
      __s=${EPOCHREALTIME%%.*}
      __us=${EPOCHREALTIME##*.}
      __now=$(( __s * 1000000000 + 10#$__us * 1000 ))
      if (( __flags & 1 )); then
        __rel=$(( __timeout - __now ))
        if (( __rel < 0 )); then __rel=0; fi
      else
        __rel=$__timeout
      fi
      __cud+=("$__ud")
      __crel+=("$__rel")
    elif (( __tag == 1 || __tag == 2 )); then
      mem_i32_load "$__p" $(( __base + 16 )) || return $?
      __fd=$R0
      __kind=${__fds[$__fd]-}
      if [[ -z $__kind || $__kind == 3 ]]; then
        __ev+=("$__ud 8 $__tag 0 0") # EBADF: unknown or directory fd
      elif (( __tag == 1 && __fd == 0 )); then
        if [[ -n $__push ]]; then
          local -a __pw=($__push)
          __ev+=("$__ud 0 1 ${#__pw[@]} 0") # pushed-back bytes are readable
        else
          __wait+=("$__ud")
        fi
      elif (( __tag == 1 && __kind == 2 )); then
        local -n __buf=${__p}wbuf${__fd}
        __avail=$(( ${#__buf[@]} - ${__tell[$__fd]} ))
        if (( __avail < 0 )); then __avail=0; fi
        __ev+=("$__ud 0 1 $__avail 0")
      else
        __ev+=("$__ud 0 $__tag 1 0") # writable fd / stdout-stderr read
      fi
    else
      R0=28 # EINVAL
      return 0
    fi
  done

  # Ready fd events win: only wait when nothing is already resolvable, mirroring
  # Ruby (clock and stdin subscriptions are not consulted once any fd is ready).
  if (( ${#__ev[@]} == 0 )); then
    local __min=0 __have_clock=0 __k __to='' __fire_clocks=0
    if (( ${#__crel[@]} > 0 )); then
      __have_clock=1
      __min=${__crel[0]}
      for (( __k = 1; __k < ${#__crel[@]}; __k++ )); do
        if (( __crel[__k] < __min )); then __min=${__crel[__k]}; fi
      done
      printf -v __to '%d.%06d' $(( __min / 1000000000 )) $(( __min % 1000000000 / 1000 ))
    fi
    if (( ${#__wait[@]} > 0 )); then
      local __ch __ord __rc __line __kk
      if [[ -t 0 ]]; then
        if (( __have_clock )); then
          IFS= read -r -t "$__to" __line
          __rc=$?
        else
          IFS= read -r __line
          __rc=$?
        fi
      elif (( __have_clock )); then
        IFS= read -r -d '' -n 1 -t "$__to" __ch
        __rc=$?
        __line=''
      else
        IFS= read -r -d '' -n 1 __ch
        __rc=$?
        __line=''
      fi
      if [[ -n $__line ]] || { [[ -t 0 ]] && (( __rc == 0 )); }; then
        # tty: buffer the whole line; the stripped newline is restored unless
        # this was EOF (or timeout) without a delimiter.
        for (( __kk = 0; __kk < ${#__line}; __kk++ )); do
          printf -v __ord '%d' "'${__line:__kk:1}"
          __push+=${__push:+ }
          __push+=$__ord
        done
        if (( __rc == 0 )); then
          __push+=${__push:+ }
          __push+=10
        fi
        local -a __pw2=($__push)
        for __ud in "${__wait[@]}"; do __ev+=("$__ud 0 1 ${#__pw2[@]} 0"); done
      elif (( __rc == 0 )); then
        # non-tty: one byte arrived
        if [[ -z $__ch ]]; then __ord=0; else printf -v __ord '%d' "'$__ch"; fi
        __push=$__ord
        for __ud in "${__wait[@]}"; do __ev+=("$__ud 0 1 1 0"); done
      elif (( __rc > 128 )); then
        __fire_clocks=1
      else
        # EOF: report each fd_read ready with 0 bytes so the guest's next read
        # sees EOF (Ruby's IO.select reports the closed fd readable instead;
        # either way the following fd_read returns 0).
        for __ud in "${__wait[@]}"; do __ev+=("$__ud 0 1 0 0"); done
      fi
    elif (( __have_clock )); then
      local __slp
      coproc __slp { IFS= read -r _; }
      IFS= read -rt "$__to" -u "${__slp[0]}" _
      exec {__slp[1]}>&-
      exec {__slp[0]}<&-
      wait "$__slp_PID" 2>/dev/null
      __fire_clocks=1
    fi
    if (( __fire_clocks )); then
      for (( __k = 0; __k < ${#__crel[@]}; __k++ )); do
        if (( __crel[__k] <= __min )); then
          __ev+=("${__cud[__k]} 0 0 0 0")
        fi
      done
    fi
  fi

  local __n=${#__ev[@]} __e __nbytes __ptr
  for (( __i = 0; __i < __n; __i++ )); do
    __ptr=$(( __out + __i * 32 ))
    mem_fill "$__p" "$__ptr" 0 32 || return $?
    read -r __ud __e __tag __nbytes __flags <<< "${__ev[__i]}"
    mem_i64_store "$__p" "$__ptr" "$__ud" || return $?
    mem_i32_store16 "$__p" $(( __ptr + 8 )) "$__e" || return $?
    mem_i32_store8 "$__p" $(( __ptr + 10 )) "$__tag" || return $?
    mem_i64_store "$__p" $(( __ptr + 16 )) "$__nbytes" || return $?
    mem_i32_store16 "$__p" $(( __ptr + 24 )) "$__flags" || return $?
  done
  mem_i32_store "$__p" "$__nevents_ptr" "$__n" || return $?
  R0=0
  return 0
}
