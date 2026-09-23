---
name: user-ken-approves-once-then-expects-standing-default
description: Ken asks for explicit approval before a first destructive/consequential action (e.g. the first git commit), but once granted treats it as a standing default going forward — the librarian should write the new default into ROADMAP's Standing rules immediately, not re-flag it as pending each session.
metadata:
  type: user
---

`RESUME.md` carried "a first commit has been offered and not yet asked for"
across at least one prior session (2026-09-22) while the repo sat untracked.
On 2026-09-23 Ken approved it, and the correct filing was not "first commit
done, still need to ask before the next one" but a **standing rule**:
commit after every passing change from here forward, recorded in
`ROADMAP.md`'s Standing rules section, not just noted as a one-off event in
`SESSION_LOG.md`.

**Why:** this mirrors the global `CLAUDE.md` pattern for subagent dispatch —
Ken drew a hard line about a background-session misreading a one-time
clearance as a permanent restriction; the mirror-image mistake here would
be treating a one-time approval as needing to be re-asked every session.
Both failures come from not updating the standing-rule record when a
one-time gate opens.

**How to apply:** when a session log entry reports "Ken approved X" for
something that was previously gated behind explicit approval, check whether
X is a recurring action (commits, a class of dispatch, a workflow step). If
so, promote it to a Standing rule / CLAUDE.md-adjacent project convention
in the same filing, not just a dated fact in the session log — the next
session should not have to re-derive "are we allowed to commit now?" from
git history.
