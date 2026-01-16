#!/bin/bash
# profiling slint apps with perf DWARF mode does not work. It hangs and I don't know why
# command looks like this. We want to remove the `--call-graph dwarf` part
#/usr/bin/perf record --freq=997 --call-graph dwarf -q -o /tmp/clion11634157358406699199perf /home/atomr/Documents/code/RustProjects/slint-slider-test/target/debug/slint-rust-template

new_args=()
arg_index=0
for arg in "$@"; do
  # Skip the argument at index 2 (using 1-based indexing for this logic)
  if [[ "$arg_index" -ne 2 && "$arg_index" -ne 3 ]]; then
    new_args+=("$arg")
  fi
  arg_index=$((arg_index + 1))
done

echo "Forwarding arguments:" "${new_args[@]}"
/usr/bin/perf "${new_args[@]}"
