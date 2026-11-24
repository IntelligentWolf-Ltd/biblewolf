#!/usr/bin/env python3
"""Convert scrollmapper/bible_databases JSON (formats/json/<ID>.json) into the
compact gzipped TSV the app embeds.

Input schema (verified against the downloaded files):
  {"translation": "<ID>: <title>", "books": [{"name", "chapters": [{"chapter", "verses": [{"verse", "text"}]}]}]}

Output: first line  "#<ID>\t<title>"
        then lines  "<book_index 0-65>\t<chapter>\t<verse>\t<text>"
"""
import gzip, json, sys, os

# Upstream titles that describe markup the plain-text "text" field doesn't
# carry (e.g. the KJV file has no Strong's numbers in its verse text).
TITLE_OVERRIDES = {
    "KJV": "King James Version (1769)",
}

def convert(src, dst_dir):
    d = json.load(open(src, encoding="utf-8"))
    tid, _, title = d["translation"].partition(":")
    tid, title = tid.strip(), title.strip()
    title = TITLE_OVERRIDES.get(tid, title)
    books = d["books"]
    if len(books) != 66:
        sys.exit(f"{src}: expected 66 books, got {len(books)}")
    out = [f"#{tid}\t{title}"]
    for bi, b in enumerate(books):
        for c in b["chapters"]:
            for v in c["verses"]:
                t = v["text"]
                if "\t" in t or "\n" in t:
                    t = " ".join(t.split())
                out.append(f"{bi}\t{c['chapter']}\t{v['verse']}\t{t.strip()}")
    path = os.path.join(dst_dir, f"{tid}.tsv.gz")
    with gzip.open(path, "wt", encoding="utf-8", compresslevel=9) as f:
        f.write("\n".join(out) + "\n")
    print(path, len(out) - 1, "verses")

if __name__ == "__main__":
    dst = sys.argv[1]
    for s in sys.argv[2:]:
        convert(s, dst)
