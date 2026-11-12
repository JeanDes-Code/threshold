#!/bin/sh
# Private text scan (mature content module, plan Task 9; review M1 and M2
# of 2026-11-12).
#
# The owner's private Content Pack lives outside the repository. This scan
# reads its folder from THRESHOLD_PRIVATE_PACK and searches the tracked
# files of the work tree and the staged content for each line of 20
# characters or more of the pack's text files, as a fixed string. The text
# files are the fragments and the word list: every file but `pack.toml`
# (its lines are format, and the format page of the repository shows them),
# with `.git` folders and binary files left out. A line that the neutral
# sample pack (fixtures/packs/test-pack) also holds is format, not private
# text, and is left out. A hit prints its file and line number, never the
# text, and the scan exits 1. A tool error stops the scan with exit 2 and a
# plain message that holds no pack text. With the variable unset, the scan
# says so in one line and does nothing else.
#
# The repository holds no word list: the only list is the owner's pack.
# The pre-commit hook (.githooks/pre-commit) runs this script.

set -u

fail() {
    echo "private_text_scan: $1" >&2
    exit 2
}

pack="${THRESHOLD_PRIVATE_PACK:-}"
if [ -z "$pack" ]; then
    echo "private_text_scan: THRESHOLD_PRIVATE_PACK is not set; skipped"
    exit 0
fi
[ -d "$pack" ] || fail "THRESHOLD_PRIVATE_PACK is not a folder"
# A relative folder, or a link to one, is resolved before the scan moves
# to the repository root.
pack=$(cd "$pack" && pwd -P) || fail "THRESHOLD_PRIVATE_PACK cannot be opened"
root=$(git rev-parse --show-toplevel 2>/dev/null) || fail "run it inside the repository"
cd "$root" || fail "the repository root cannot be opened"

# The text of the scanned files of folder $1, each file followed by a line
# end: every regular file but `pack.toml`, outside `.git` folders, that is
# text (grep -I reads a file with a NUL byte as binary, and an empty file
# holds nothing). A file that cannot be read is a tool error: the status is
# not 0. The tools' own messages can name a file of the pack, so they are
# not shown; the caller prints a plain message instead.
text_of() {
    find "$1" -name .git -prune -o -type f ! -name pack.toml -exec sh -c '
        for f do
            LC_ALL=C grep -qI . "$f"
            case $? in
                0) cat "$f" || exit 2; echo ;;
                1) ;;
                *) exit 2 ;;
            esac
        done' sh {} + 2>/dev/null
}

private_text=$(text_of "$pack") || fail "a file of the private pack cannot be read"
sample_text=""
if [ -d fixtures/packs/test-pack ]; then
    sample_text=$(text_of fixtures/packs/test-pack) || fail "the sample pack cannot be read"
fi

# Every line of 20 characters or more of the private text, trimmed, once,
# without the lines of the sample pack. They stay in memory: no file holds
# them.
patterns=$(
    printf '%s\n' "$private_text" | SAMPLE="$sample_text" awk '
        function trim(s) { sub(/^[ \t]+/, "", s); sub(/[ \t\r]+$/, "", s); return s }
        BEGIN {
            n = split(ENVIRON["SAMPLE"], lines, "\n")
            for (i = 1; i <= n; i++) seen[trim(lines[i])] = 1
        }
        { line = trim($0) }
        length(line) >= 20 && !(line in seen) && !(line in done) { done[line] = 1; print line }'
) || fail "awk failed"
[ -n "$patterns" ] || exit 0

# `file:line` of each hit. `git grep --null` ends the file name and the
# line number with a NUL, so a file name with a colon stays whole; `tr` turns the NUL into \001,
# since a shell variable cannot hold a NUL. The matched text is cut off
# before anything prints. The last line of `search` is git grep's status:
# 1 is "no match"; any status but 0 and 1 is a tool error.
sep=$(printf '\001')
search() {
    {
        printf '%s\n' "$patterns" | git grep "$@" -n --null -I -F -f - -- 2>/dev/null
        echo "status $?"
    } | tr '\000' '\001'
}
hits_of() {
    out=$(search "$@") || fail "git grep could not run"
    status=$(printf '%s\n' "$out" | tail -n 1)
    case $status in
        "status 0" | "status 1") ;;
        *) fail "git grep failed ($status)" ;;
    esac
    printf '%s\n' "$out" | sed '$d' | sed -n "s/${sep}\([0-9][0-9]*\)${sep}.*\$/:\1/p"
}
work=$(hits_of) || exit 2
staged=$(hits_of --cached) || exit 2

hits=$(
    {
        [ -z "$work" ] || printf '%s\n' "$work" | sed 's/^/work tree: /'
        [ -z "$staged" ] || printf '%s\n' "$staged" | sed 's/^/staged: /'
    } | sort -u
)

if [ -n "$hits" ]; then
    echo "private_text_scan: a line of the private pack is in the repository:" >&2
    echo "$hits" >&2
    exit 1
fi
exit 0
