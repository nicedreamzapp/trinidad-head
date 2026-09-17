#!/bin/bash
# Install the Trinidad Head helper commands into ~/Scripts/trinidad-head.
set -e
cd "$(dirname "$0")"
mkdir -p "$HOME/Scripts/trinidad-head"
for f in th-type th-self-id th-alive th-open; do
    install -m 755 "$f" "$HOME/Scripts/trinidad-head/$f"
done
echo "installed helpers into ~/Scripts/trinidad-head"
