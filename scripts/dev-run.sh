#!/usr/bin/env bash
# Cargo runner: seed dummy data if the database is empty, then run the binary.
# Debug app binary only; release and test binaries are run as-is.
set -e
case "$1" in
  */debug/cv-webpage) "$1" seed --if-empty ;;
esac
exec "$@"
