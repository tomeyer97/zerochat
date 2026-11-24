#!/usr/bin/env bash
# A fake `zeroshot` for tests/cli_zeroshot.rs: just the commands and output zsf's ZeroshotCli
# reads (crates/zsf/src/zeroshot.rs). It records what it was called with in $FAKE_ZS_DIR.
case "$1" in
  run)
    # `zeroshot run --help`: the flags zsf looks for before it uses them.
    if [ "$2" = "--help" ]; then
      echo "  -d, --detach  Return after submission"
      echo "      --submission-key <KEY>"
      exit 0
    fi
    # The n-th launch: its arguments, its working directory, and some work in that directory.
    n=$(( $(cat "$FAKE_ZS_DIR/count" 2>/dev/null || echo 0) + 1 ))
    echo $n > "$FAKE_ZS_DIR/count"
    printf '%s\n' "$@" > "$FAKE_ZS_DIR/args-$n"
    pwd > "$FAKE_ZS_DIR/cwd-$n"
    echo "work $n" > "work-$n.txt"
    # The submitted run, as `zeroshot run --detach` prints it.
    echo '{"runId":"01a0fake-0000-7000-8000-00000000000'$n'","title":"x"}'
    ;;
  resume)
    echo '{"runId":"'"$2"'-resumed","resumedFrom":"'"$2"'"}'
    ;;
  status)
    # Which state directory zsf pointed Zeroshot at when it looked at run $2.
    echo "$ZEROSHOT_STATE_DIR" > "$FAKE_ZS_DIR/observed-$2"
    echo '{}'
    ;;
  force-stop)
    echo '{}'
    ;;
  *)
    exit 2
    ;;
esac
