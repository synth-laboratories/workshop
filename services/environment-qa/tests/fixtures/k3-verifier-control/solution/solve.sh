#!/bin/sh
set -eu
printf '%s\n' 'import sys' 'print(int(sys.argv[1]) ** 2)' > /app/square.py
