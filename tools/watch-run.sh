#!/usr/bin/env bash
# Live status monitor for a background arena/train run.
# Usage: tools/watch-run.sh <output-file> [process-name-pattern]
#   process-name-pattern defaults to "target/release/arena"
# Refreshes in place every 2s; prints the final result table and exits when done.

OUT="${1:?usage: watch-run.sh <output-file> [proc-pattern]}"
PAT="${2:-target/release/arena}"

while true; do
  printf '\033[2J\033[H'                      # clear screen, cursor home
  echo "▶ watching: $OUT"
  echo "  $(date '+%H:%M:%S')"
  echo "------------------------------------------------------------"

  # process line: pid, cpu%, wall-elapsed, cumulative cpu-time.
  # Match the actual binary, not the shell wrapper or this grep; pick busiest.
  line=$(ps -axo pid,pcpu,etime,time,command \
    | grep -F "$PAT" | grep -v -e ' -c ' -e 'grep' -e 'watch-run' \
    | sort -k2 -nr | head -1)
  if [ -n "$line" ]; then
    set -- $line
    echo "  status   : RUNNING (pid $1)"
    echo "  cpu      : $2%   (≈$(echo "$2/100" | bc -l | cut -c1-4) cores)"
    echo "  wall     : $3"
    echo "  cpu-time : $4"
  else
    echo "  status   : not running (finished or not started)"
  fi
  echo "------------------------------------------------------------"

  if [ -f "$OUT" ]; then
    # Completion markers: arena prints "elo", train/selfplay print "wrote".
    if grep -qiE '\belo\b|^wrote |panic|error\[' "$OUT"; then
      echo "  ✓ DONE — results:"
      echo
      cat "$OUT"
      exit 0
    else
      echo "  latest output (per-game progress, if any):"
      tail -8 "$OUT" | sed 's/^/    /'
    fi
  else
    echo "  (output file not created yet)"
  fi

  sleep 2
done
