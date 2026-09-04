#!/bin/sh
# Writes the manifest the streaming mode reads, from an extracted disc.
#
#   ./manifest.sh /path/to/extract > /path/to/served/manifest.json
#
# Paths are relative to the directory given, which is what the page appends to
# its `?files=` base.
set -eu
cd "${1:?usage: manifest.sh <extracted disc>}"
find . -type f | sed 's|^\./||' | sort | awk '
  BEGIN { printf "[" ; sep = "" }
        { printf "%s\n  \"%s\"", sep, $0 ; sep = "," }
  END   { printf "\n]\n" }
'
