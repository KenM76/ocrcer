# Font licence inventory — chunk 1

**Method.** Enumerated `C:\Windows\Fonts` (544 files) and searched installed
applications (Android Studio's bundled JetBrains Runtime, SOLIDWORKS'
`spiop` PDF-export resource set, Scribus, Eclipse/OFBiz, BRL-CAD, VcXsrv,
Ghostscript, Adobe Acrobat) for permissively-licensed families. Licences
were established from embedded TrueType `name` table entries (read with
`fontTools`, the strongest available source — a bundled licence statement)
where present, cross-checked against upstream project pages for two web
searches and one repo fetch. Character coverage was **measured**, not
assumed: `fontTools` cmap inspection against the charset's symbol set
(`Ø⌀°±×÷√≤≥≈≠Ωµ¼½¾§¶†‡©®™‰–—‚„…¢£€¥`) and the Latin-1 accented block, for
every candidate face.

**Result: 19 faces eligible-present**, all with real paths on this
machine, all OFL-1.1 or Apache-2.0, sourced to embedded font metadata or a
canonical upstream page — see `fonts.tsv`. One more (**norm-stroke**, CC0) is
eligible but must be acquired (`eligible-absent`). Three (**osifont**,
**Terminus Font**, **DejaVu Sans**) are `needs-operator` — their licences are
real and permissive but do not cleanly satisfy rule 2's OFL/Apache/public-domain
bar, and that call was pushed up rather than guessed. Roughly 470 remaining Windows-installed faces are `proprietary`,
represented by a sample (Arial, Times New Roman, Calibri, Segoe UI, Tahoma,
Verdana, Cambria, Consolas, Courier New) rather than catalogued individually.

**Typographic coverage achieved:** a Times-like serif (Liberation Serif) and
a second serif shape (Noto Serif); a Helvetica/Arial-like grotesque
(Liberation Sans, Roboto); several humanist sans (Open Sans Condensed, Lato,
Inter, Noto Sans, PT Sans); monospace (Liberation Mono, Roboto Mono,
JetBrains Mono, Fira Code, Cascadia Code/Mono, PT Mono, Inconsolata); and
condensed (Open Sans Condensed, Roboto Condensed). That is every category
`ARCHITECTURE.md` §4 asks for **except** genuine CAD single-stroke
lettering.

**The gap, stated plainly: no true ISO 3098 lettering face is present on
this machine, and the obvious candidate is not eligible.** `osifont` — the
face `ARCHITECTURE.md` §10 already names as if it were cleared — turns out
on inspection to be GPL-3.0/LGPL-3.0 with a font-linking exception, not
OFL/Apache/public-domain. That is a real discrepancy between the
architecture doc and what this inventory found, flagged in `fonts.tsv` for
an operator decision rather than silently resolved either way. The clean
fix is **`norm-stroke`** (github.com/octycs/norm-stroke), a genuine ISO
3098 Type B single-line face released CC0 — verified by fetching the repo
— but it is not installed anywhere and must be acquired before chunk 3.
`STIX Two Math` (OFL-1.1, already present) is a partial mitigant: it is the
only bundled face that covers the full technical-symbol set, but it is a
math font, not a CAD lettering shape.

**Character-level risk, measured:** the diameter sign (U+2300 ⌀) is missing
from every eligible face on this machine except Fira Code — including
Liberation Sans/Serif/Mono, Roboto, Noto Sans, Lato, Inter, JetBrains Mono,
Cascadia Code/Mono. Since a class with zero prototypes in the bank cannot be
recognised at all (rule 8), ⌀ is currently a class the bank would fail on
outright without either Fira Code, STIX Two Math, or an acquired face
supplying it. Noto Sans and Inconsolata also lack √, ≤, ≥, ≈, ≠; Lato lacks
Ω. All Latin-1 accented letters are present in every eligible face — no risk
there.

**Acquisition shortlist before chunk 3:**
1. `norm-stroke` (CC0) — closes the CAD single-stroke lettering gap, and is
   the only unambiguously clear face on the list.
2. `DejaVu Sans` — a broad-Unicode backstop, and the widest coverage of the
   charset's rarer symbols of anything surveyed. Its licence is Bitstream
   Vera-derived: permissive, but carrying a name-change clause and a
   no-sale-alone clause, so it is neither OFL, Apache nor public domain. It
   needs an operator decision before acquisition, not after.

**Needs an operator decision:**
- Whether `osifont`'s GPL+font-exception licence is acceptable for this
  project, or whether `norm-stroke` replaces it and `ARCHITECTURE.md` §10's
  example list gets corrected.
- Whether `Terminus Font` (bitmap, version-and-thus-licence unconfirmed) is
  worth pursuing at all, given monospace is already well covered.
