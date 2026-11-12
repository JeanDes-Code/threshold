#!/bin/sh
# Private text scan (mature content module, plan Task 9).
#
# The owner's private Content Pack lives outside the repository. This scan
# reads its folder from THRESHOLD_PRIVATE_PACK and searches the tracked
# files of the work tree and the staged content for each of its lines of
# 20 characters or more, as a fixed string. A line that the neutral sample
# pack (fixtures/packs/test-pack) also holds is format, not private text,
# and is left out. A hit prints its file and line number, never the text,
# and the scan exits 1. With the variable unset, the scan does nothing.
#
# The repository holds no word list: the only list is the owner's pack.
# The pre-commit hook (.githooks/pre-commit) runs this script.

set -u

pack="${THRESHOLD_PRIVATE_PACK:-}"
[ -n "$pack" ] || exit 0
if [ ! -d "$pack" ]; then
    echo "private_text_scan: THRESHOLD_PRIVATE_PACK is not a folder" >&2
    exit 2
fi
root=$(git rev-parse --show-toplevel 2>/dev/null) || {
    echo "private_text_scan: run it inside the repository" >&2
    exit 2
}
cd "$root" || exit 2

# Every line of 20 characters or more of the private pack, trimmed, without
# the lines of the sample pack. Printed to a pipe only: no file holds them.
lines() {
    find -H "$pack" -type f -exec cat {} + 2>/dev/null |
        awk '{ sub(/^[ \t]+/, ""); sub(/[ \t\r]+$/, ""); if (length($0) >= 20) print }' |
        sort -u |
        awk -v sample="fixtures/packs/test-pack" '
            BEGIN {
                cmd = "find \"" sample "\" -type f -exec cat {} + 2>/dev/null"
                while ((cmd | getline line) > 0) {
                    sub(/^[ \t]+/, "", line); sub(/[ \t\r]+$/, "", line)
                    seen[line] = 1
                }
                close(cmd)
            }
            !($0 in seen)'
}

# `file:line` of each hit; the matched text is cut off before it prints.
where() {
    sed 's/^\([^:]*:[0-9][0-9]*\):.*$/\1/'
}

hits=$(
    {
        lines | git grep -n -I -F -f - -- 2>/dev/null | where | sed 's/^/work tree: /'
        lines | git grep --cached -n -I -F -f - -- 2>/dev/null | where | sed 's/^/staged: /'
    } | sort -u
)

if [ -n "$hits" ]; then
    echo "private_text_scan: a line of the private pack is in the repository:" >&2
    echo "$hits" >&2
    exit 1
fi
exit 0
