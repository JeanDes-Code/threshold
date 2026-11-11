"""Common prefix of consecutive prompts in a Campaign's debug log.

Ollama reuses a prompt up to its first changed byte (ADR 0001). This shows
where consecutive prompts of one step stop sharing. It opens the file read
only; run it on a copy of the Campaign file all the same.

Usage: python3 -I scripts/qa/prompt_prefix.py <campaign.sqlite> [step ...]
Prints, for each pair of consecutive calls of the given steps (default:
propose), the length of each prompt, the shared prefix (chars, and tokens
at about 4 chars per token) and what follows the first differing byte.
"""
import sqlite3
import sys

db = sys.argv[1]
steps = sys.argv[2:] or ["propose"]
con = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
q = "select id, op_id, step, prompt from debug_log where step in (%s) order by id" % ",".join("?" * len(steps))
rows = [r for r in con.execute(q, steps) if r[3]]
# One row per op_id: the first call (a retry of the same op shares its prompt).
seen, calls = set(), []
for rid, op, step, prompt in rows:
    if (op, step) in seen:
        continue
    seen.add((op, step))
    calls.append((rid, op, step, prompt))

def common(a, b):
    n = min(len(a), len(b))
    i = 0
    while i < n and a[i] == b[i]:
        i += 1
    return i

total_shared = total_len = 0
for k in range(1, len(calls)):
    a, b = calls[k - 1][3], calls[k][3]
    i = common(a, b)
    total_shared += i
    total_len += len(b)
    line_start = b.rfind("\n", 0, i) + 1
    ctx = b[line_start:i + 60].split("\n")[0]
    print(f"call {k:2d} (log {calls[k][0]:3d}): len {len(b):6d}  shared {i:6d} chars "
          f"(~{i // 4:5d} tok, {100 * i / len(b):5.1f}%)  diverges in: {ctx[:110]!r}")
print(f"mean shared fraction: {100 * total_shared / max(total_len, 1):.1f}%")
