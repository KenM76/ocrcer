---
name: feedback-no-shell-label-unverified
description: When a librarian dispatch has no Bash/shell tool, label environment/tooling figures (disk space, clippy/rustfmt output, /usage) as reported-not-independently-verified rather than asserting them as checked.
metadata:
  type: feedback
---

This role's tool loadout varies by dispatch — some sessions get Read/Write/
Edit/Glob/Grep/WebSearch/WebFetch only, with no Bash tool to run `df`,
`cargo clippy`, or check `/usage` directly. Hard rule 7 (never assert
environment/budget state you have not checked) still applies in that case:
the correct move is to file the figure as *reported this session, not
independently re-verified — no shell available in this dispatch*, not to
either invent verification or silently drop the figure.

**Why:** first hit 2026-09-23, filing a disk-pressure incident (D: hit
100% free, agent deleted two scratch files, reported 38 GB free afterward)
and a code-health snapshot (45 clippy warnings, 63 files of rustfmt drift)
that the parent session had already measured but this dispatch had no way
to re-check. Filed both with an explicit "unverified from here" caveat in
`ROADMAP.md` and `RESUME.md` rather than stating them flatly.

**How to apply:** before filing any environment-state figure, check whether
a shell/Bash tool is actually present in this dispatch's toolset. If yes,
verify per hard rule 7. If no, file the figure but flag it as reported and
unverified, and name what would verify it (e.g. "confirm current D: free
space" as a `RESUME.md` §5 item). Do not silently upgrade a reported figure
to a checked one just because it came from a credible-sounding session
report — the parent agent's report is not the same thing as this dispatch
having looked.
