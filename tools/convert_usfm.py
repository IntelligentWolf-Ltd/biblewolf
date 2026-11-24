#!/usr/bin/env python3
"""Convert an eBible.org USFM Bible (https://ebible.org/Scriptures/<id>_usfm.zip,
unzipped) into the gzipped TSV the app embeds, keeping two kinds of inline
markup as control characters:

  \\x01 ... \\x02   words of Jesus      (USFM \\wj ... \\wj*)   -> red letters
  \\x03 ... \\x04   translator-supplied (USFM \\add ... \\add*) -> italics
  \\x05 ... \\x06   title / heading     (USFM \\d psalm titles, \\s e.g. Psalm 119's
                    ALEPH, BETH...), placed before the next verse; a heading at
                    the end of a book (KJV epistle subscriptions) follows the
                    last verse

Output: first line  "#<ID>\\t<title>"
        then lines  "<book_index 0-65>\\t<chapter>\\t<verse>\\t<text>"

Usage: convert_usfm.py <usfm_dir> <out_dir> <ID> "<title>"

The parser is strict: any marker it doesn't know is an error, so markup can't
be silently dropped or leaked into the text.
"""
import gzip, os, re, sys

# Standard USFM book codes in our canonical 66-book order (bible.rs BOOKS).
BOOK_CODES = (
    "GEN EXO LEV NUM DEU JOS JDG RUT 1SA 2SA 1KI 2KI 1CH 2CH EZR NEH EST JOB PSA PRO "
    "ECC SNG ISA JER LAM EZK DAN HOS JOL AMO OBA JON MIC NAM HAB ZEP HAG ZEC MAL "
    "MAT MRK LUK JHN ACT ROM 1CO 2CO GAL EPH PHP COL 1TH 2TH 1TI 2TI TIT PHM HEB "
    "JAS 1PE 2PE 1JN 2JN 3JN JUD REV"
).split()
assert len(BOOK_CODES) == 66

WJ_OPEN, WJ_CLOSE, ADD_OPEN, ADD_CLOSE = "\x01", "\x02", "\x03", "\x04"
TITLE_OPEN, TITLE_CLOSE = "\x05", "\x06"

# Whole-line markers whose text is not verse text (titles, headings, etc.).
HEADING = re.compile(r"^\\(id|ide|h|toc\d|mt\d?|ms\d?|mr|sr|r|sp|cl|rem|sts|usfm|cp)\b")
# Psalm superscriptions and in-text headings: kept, attached to a verse.
TITLE = re.compile(r"^\\(?:d|s\d?)\s+(.*)$")
# Paragraph / poetry markers: just separators inside verse text.
PARA = re.compile(r"\\(p|m|pi\d?|mi|nb|pc|pm|pmo|pmc|pmr|q\d?|qr|qc|qm\d?|li\d?|b|ph\d?)(?=\s|$)")
NOTE = re.compile(r"\\(f|fe|x) .*?\\\1\*", re.S)
WORD = re.compile(r"\\(\+?)w (.*?)\\\1w\*", re.S)
# Character styles we keep only the text of. In USFM the opening marker owns
# one following space ("\\nd LORD\\nd* God"); the closing marker owns none.
STYLE = r"(nd|tl|sc|bk|qs|k|bd|it|em|pn|sig|no|ord)"
STRIP_OPEN = re.compile(r"\\\+?" + STYLE + r" ")
STRIP_CLOSE = re.compile(r"\\\+?" + STYLE + r"\*")


def clean_inline(text: str) -> str:
    text = NOTE.sub("", text)
    text = WORD.sub(lambda m: m.group(2).split("|", 1)[0], text)
    text = text.replace("\\+wj*", WJ_CLOSE).replace("\\wj*", WJ_CLOSE)
    text = re.sub(r"\\\+?wj\s?", WJ_OPEN, text)
    text = text.replace("\\+add*", ADD_CLOSE).replace("\\add*", ADD_CLOSE)
    text = re.sub(r"\\\+?add\s?", ADD_OPEN, text)
    text = STRIP_OPEN.sub("", text)
    text = STRIP_CLOSE.sub("", text)
    text = PARA.sub(" ", text)
    leftover = re.search(r"\\[a-z+0-9*]+", text)
    if leftover:
        raise ValueError(f"unknown USFM marker {leftover.group(0)!r} in: {text[:120]!r}")
    return text


def tidy(text: str) -> str:
    text = text.replace("\u00b6", " ")  # KJV paragraph signs
    text = " ".join(text.split())
    # Markers hug the words they wrap: "x \x01 y" -> "x \x01y", "y \x02 z" -> "y\x02 z"
    for o in (WJ_OPEN, ADD_OPEN, TITLE_OPEN):
        text = text.replace(o + " ", o)
    for c in (WJ_CLOSE, ADD_CLOSE, TITLE_CLOSE):
        text = text.replace(" " + c, c)
    # No space before punctuation that ends up next to a closing marker.
    text = re.sub(r" ([,.;:!?)])", r"\1", text)
    # Merge spans split only by a space (the WEB tags Jesus' words per phrase).
    for o, c in ((WJ_OPEN, WJ_CLOSE), (ADD_OPEN, ADD_CLOSE)):
        text = text.replace(c + " " + o, " ")
    # Drop empty spans.
    text = text.replace(WJ_OPEN + WJ_CLOSE, "").replace(ADD_OPEN + ADD_CLOSE, "")
    return text.strip()


def balanced(text: str) -> bool:
    for o, c in ((WJ_OPEN, WJ_CLOSE), (ADD_OPEN, ADD_CLOSE), (TITLE_OPEN, TITLE_CLOSE)):
        depth = 0
        for ch in text:
            if ch == o:
                depth += 1
                if depth > 1:
                    return False
            elif ch == c:
                depth -= 1
                if depth < 0:
                    return False
        if depth != 0:
            return False
    return True


def parse_book(path):
    raw = open(path, encoding="utf-8-sig").read()
    m = re.search(r"^\\id (\w+)", raw, re.M)
    if not m:
        raise ValueError(f"{path}: no \\id line")
    code = m.group(1)
    if code not in BOOK_CODES:
        return code, None
    verses = {}  # (chapter, verse) -> text
    chapter, cur = 0, None
    pending_title = None
    # Notes can span lines; remove them before splitting into lines.
    raw = NOTE.sub("", raw)
    for line in raw.splitlines():
        line = line.strip()
        if not line:
            continue
        if HEADING.match(line):
            cur = None  # heading text never belongs to a verse
            continue
        tm = TITLE.match(line)
        if tm:
            pending_title = tm.group(1)
            cur = None
            continue
        cm = re.match(r"^\\c (\d+)", line)
        if cm:
            chapter, cur = int(cm.group(1)), None
            continue
        # A line may hold several verses: split on \v markers.
        parts = re.split(r"\\v (\d+)(?:-\d+)?\s?", line)
        if parts[0].strip() and cur is not None:
            verses[cur] += " " + parts[0]
        for i in range(1, len(parts), 2):
            cur = (chapter, int(parts[i]))
            if cur in verses:
                raise ValueError(f"{code} {cur}: duplicate verse")
            verses[cur] = parts[i + 1]
            if pending_title is not None:
                verses[cur] = TITLE_OPEN + pending_title + TITLE_CLOSE + " " + verses[cur]
                pending_title = None
    if pending_title is not None:
        last = max(verses)
        verses[last] += " " + TITLE_OPEN + pending_title + TITLE_CLOSE
    out = {}
    wj_open = False
    for key in sorted(verses):
        t = clean_inline(verses[key])
        # A words-of-Jesus span left open at a verse end continues into the
        # next verse: close it here and reopen there so every verse is balanced.
        if wj_open:
            t = WJ_OPEN + t
        opens, closes = t.count(WJ_OPEN), t.count(WJ_CLOSE)
        wj_open = opens > closes
        if wj_open:
            t += WJ_CLOSE
        t = tidy(t)
        if not balanced(t):
            raise ValueError(f"{code} {key}: unbalanced markup: {t[:160]!r}")
        out[key] = t
    return code, out


def main():
    src, dst, tid, title = sys.argv[1:5]
    books = {}
    for name in sorted(os.listdir(src)):
        if not name.lower().endswith(".usfm"):
            continue
        code, verses = parse_book(os.path.join(src, name))
        if verses is None:
            print(f"skipping {name} ({code}): not in the 66-book canon")
            continue
        books[code] = verses
    missing = [c for c in BOOK_CODES if c not in books]
    if missing:
        sys.exit(f"missing books: {missing}")
    lines = [f"#{tid}\t{title}"]
    n = wj = 0
    for bi, code in enumerate(BOOK_CODES):
        for (c, v), t in sorted(books[code].items()):
            if "\t" in t or "\n" in t:
                sys.exit(f"{code} {c}:{v}: tab/newline in text")
            lines.append(f"{bi}\t{c}\t{v}\t{t}")
            n += 1
            wj += WJ_OPEN in t
    path = os.path.join(dst, f"{tid}.tsv.gz")
    with gzip.open(path, "wt", encoding="utf-8", compresslevel=9) as f:
        f.write("\n".join(lines) + "\n")
    print(f"{path}: {n} verses, {wj} with words of Jesus")


if __name__ == "__main__":
    main()
