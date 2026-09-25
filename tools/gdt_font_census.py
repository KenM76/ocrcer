#!/usr/bin/env python3
"""GD&T / hole-callout font census (read-only). See report for method notes.

For every font file under the same roots the 2026-09-22 U+2300 audit used,
record which of the 27 GD&T/callout codepoints have a cmap entry that maps
to a glyph with non-empty outlines (RecordingPen has >0 recorded segments).
Classify each face's licence from name table IDs 0/8/13/14 plus known
upstream licence for a short allow-list of families this project already
carries a licence finding for (fonts.tsv). Anything else is "unclear".

Usage: python tools/gdt_font_census.py [--roots DIR ...] > gdt_census_raw.tsv
Report: docs/measurements/2026-09-25_gdt_font_census.md
"""
import argparse
import os
import sys
import hashlib
from fontTools.ttLib import TTFont
from fontTools.pens.recordingPen import RecordingPen

CODEPOINTS = [
    0x23E4, 0x23E5, 0x25CB, 0x232D, 0x2312, 0x2313, 0x27C2, 0x2220, 0x2225,
    0x2316, 0x25CE, 0x232F, 0x2197, 0x2330, 0x24BB, 0x24C1, 0x24C2, 0x24C5,
    0x24C8, 0x24C9, 0x2334, 0x2335, 0x2332, 0x2333, 0x2331, 0x21A7, 0x25A1,
]

def default_roots():
    env = os.environ
    return [
        os.path.join(env.get("WINDIR", r"C:\Windows"), "Fonts"),
        os.path.join(env.get("LOCALAPPDATA", ""), "Microsoft", "Windows", "Fonts"),
        env.get("ProgramFiles", r"C:\Program Files"),
        env.get("ProgramFiles(x86)", r"C:\Program Files (x86)"),
    ]


EXTS = (".ttf", ".otf", ".ttc", ".otc")

# Known upstream licence for families this project has already sourced a
# licence finding for (fonts.tsv / 2026-09-22 audit). name-table text alone
# is frequently silent or ambiguous (TTC members, subsetted copies), so this
# list carries forward findings already made rather than re-deriving them.
KNOWN_CLEAN_FAMILIES = {
    "liberation sans", "liberation serif", "liberation mono", "noto sans",
    "noto serif", "open sans condensed", "roboto", "roboto condensed",
    "roboto mono", "lato", "pt sans", "pt mono", "fira code", "inter",
    "jetbrains mono", "cascadia code", "cascadia mono", "stix two math",
    "noto sans symbols", "noto sans symbols2", "noto sans math", "dejavu sans",
    "dejavu sans mono", "dejavu serif",
}
KNOWN_BARRED_FAMILIES = {
    "arial", "times new roman", "calibri", "segoe ui", "segoe ui symbol",
    "tahoma", "verdana", "cambria", "cambria math", "consolas", "courier new",
    "ds iso 1", "geniso", "isocpeur", "isocteur", "gost common", "myriad cad",
    "complex", "simplex", "aigdt", "amgdt", "osifont",
    # URW++ Core35 set bundled with Scribus (URWFonts-1.41): AFPL per
    # fonts.tsv row "Nimbus Sans L / Nimbus Roman No9 L / Nimbus Mono L"
    # (local License.htm read in full: "Aladdin Free Public License").
    # AFPL is explicitly not one of rule 2's three permitted licences.
    "a028 extrabold", "a028 medium", "a030", "antiqueolive", "centuryschl",
    "clarendonurwbolcon", "coronet", "garamondno8", "lettergothic",
    "mauritius", "nimbusmonl", "nimbusmono", "nimbusromno9l",
    "nimbusromanno4", "nimbusromanno9", "nimbussanl", "nimbussanlcon",
    "u001", "u001con", "urwbookmanl", "urwchancerylmed", "urwclassico",
    "urwgothicl", "urwgothicldem", "urwpalladiol",
    "hyswlongfangsong",
}

NAME_IDS = (0, 8, 13, 14)


def iter_font_files(roots):
    for root in roots:
        if not os.path.isdir(root):
            print(f"# root absent, skipped: {root}", file=sys.stderr)
            continue
        for dirpath, _dirnames, filenames in os.walk(root):
            for fn in filenames:
                if fn.lower().endswith(EXTS):
                    yield os.path.join(dirpath, fn)


def licence_strings(ttfont):
    out = []
    try:
        name_table = ttfont["name"]
    except KeyError:
        return out
    for rec in name_table.names:
        if rec.nameID in NAME_IDS:
            try:
                out.append(rec.toUnicode())
            except Exception:
                pass
    return out


def classify_licence(family, strings):
    fam_l = (family or "").strip().lower()
    if fam_l in KNOWN_CLEAN_FAMILIES:
        return "clean", "known-family (fonts.tsv / 2026-09-22 audit)"
    if fam_l in KNOWN_BARRED_FAMILIES:
        return "barred", "known-family (fonts.tsv / 2026-09-22 audit)"
    blob = " ".join(strings).lower()
    if "open font license" in blob or "scripts.sil.org/ofl" in blob or "ofl-1.1" in blob:
        return "clean", "name-table OFL string"
    if "apache" in blob and "license" in blob:
        return "clean", "name-table Apache string"
    if "public domain" in blob or "creativecommons.org/publicdomain" in blob or "cc0" in blob:
        return "clean", "name-table public-domain string"
    if "gnu general public license" in blob or "gpl" in blob:
        return "barred", "name-table GPL string"
    if "all rights reserved" in blob or "microsoft" in blob or "monotype" in blob or "autodesk" in blob or "dassault" in blob:
        return "barred", "name-table proprietary string"
    return "unclear", "no recognised licence string, family not in known lists"


def glyph_has_ink(ttfont, glyph_name):
    try:
        glyph_set = ttfont.getGlyphSet()
        pen = RecordingPen()
        glyph_set[glyph_name].draw(pen)
        return len(pen.value) > 0
    except Exception:
        return False


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--roots", nargs="+", default=default_roots())
    args = ap.parse_args()
    print("path\tfamily\tsubfamily\tlicence\tlicence_basis\tcodepoints_with_ink")
    n_files = 0
    n_ok = 0
    for path in iter_font_files(args.roots):
        n_files += 1
        try:
            collection_size = 1
            if path.lower().endswith((".ttc", ".otc")):
                from fontTools.ttLib import TTCollection
                coll = TTCollection(path, lazy=True)
                collection_size = len(coll.fonts)
                fonts_to_check = [(i, coll.fonts[i]) for i in range(collection_size)]
            else:
                fonts_to_check = [(None, TTFont(path, lazy=True, fontNumber=0))]
        except Exception as e:
            print(f"# UNREADABLE {path}: {e}", file=sys.stderr)
            continue
        for idx, ttfont in fonts_to_check:
            try:
                family = ttfont["name"].getDebugName(1) or ttfont["name"].getDebugName(16) or "?"
                subfamily = ttfont["name"].getDebugName(2) or ttfont["name"].getDebugName(17) or "?"
                cmap = ttfont.getBestCmap()
                if not cmap:
                    continue
                hits = []
                for cp in CODEPOINTS:
                    gname = cmap.get(cp)
                    if gname and glyph_has_ink(ttfont, gname):
                        hits.append(cp)
                strings = licence_strings(ttfont)
                licence, basis = classify_licence(family, strings)
                sub = f"{path}#{idx}" if idx is not None else path
                cps = ",".join(f"U+{cp:04X}" for cp in hits) if hits else ""
                print(f"{sub}\t{family}\t{subfamily}\t{licence}\t{basis}\t{cps}")
                n_ok += 1
            except Exception as e:
                print(f"# error reading font in {path}: {e}", file=sys.stderr)
    print(f"# files_seen={n_files} fonts_read_ok={n_ok}", file=sys.stderr)


if __name__ == "__main__":
    main()
