#!/usr/bin/env bash
set -euo pipefail

echo "===== timestamp ====="
date -Is
echo
echo "===== uname ====="
uname -a
echo
echo "===== free ====="
free -h
echo
echo "===== meminfo ====="
cat /proc/meminfo | egrep 'MemTotal|MemFree|MemAvailable|Buffers|Cached|SwapTotal|SwapFree'
echo
echo "===== top rss ====="
ps -eo pid,comm,rss,vsz,%mem --sort=-rss | head -20

