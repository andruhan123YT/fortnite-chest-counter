"""Write crates/app/src/i18n/<module>.rs from a numbered translation file.

    python tools/i18n_gen.py keys                 # print the English texts, numbered
    python tools/i18n_gen.py build <module> <tsv> <rule> # write the table; rule: single, one, russian, polish

The English texts and their order come from ru.rs. The .tsv holds four header lines
(`@months a|b|...`, `@months_full`, `@days`, `@days_full`, values split by `|`) and then one line per
text: its number, a space and the translation (a second space keeps a leading one). `\\n` stands for a line break. Plural keys
(`plural:match`) list their forms split by `|`.
"""

import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
I18N = ROOT / "crates" / "app" / "src" / "i18n"
FORMS = {"single": 1, "one": 2, "russian": 3, "polish": 3, "arabic": 5}


def english_keys():
    keys = []
    decoder = json.JSONDecoder()
    for line in (I18N / "ru.rs").read_text(encoding="utf-8").splitlines():
        s = line.strip()
        if s.startswith('("'):
            key, _ = decoder.raw_decode(s[1:])
            keys.append(key)
    return keys


def rust(s):
    return json.dumps(s, ensure_ascii=False)


def build(module, tsv, forms):
    keys = english_keys()
    header, text = {}, {}
    for n, line in enumerate(pathlib.Path(tsv).read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip():
            continue
        head, _, value = line.partition(" ")
        value = value.replace("\\n", "\n")
        if head.startswith("@"):
            header[head[1:]] = value.split("|")
            continue
        i = int(head)
        if i in text:
            sys.exit(f"line {n}: text {i} twice")
        text[i] = value
    problems = []
    for name, count in (("months", 12), ("months_full", 12), ("days", 7), ("days_full", 7)):
        if len(header.get(name, [])) != count:
            problems.append(f"@{name} needs {count} values")
    for i, en in enumerate(keys, 1):
        t = text.get(i)
        if t is None:
            problems.append(f"{i} missing: {en!r}")
            continue
        if set(re.findall(r"\{\d\}", en)) != set(re.findall(r"\{\d\}", t)):
            problems.append(f"{i} placeholders: {en!r} -> {t!r}")
        if en.count("\n") != t.count("\n"):
            problems.append(f"{i} line breaks: {en!r} -> {t!r}")
        if en.startswith("plural:") and len(t.split("|")) != forms:
            problems.append(f"{i} needs {forms} forms: {t!r}")
    extra = sorted(set(text) - set(range(1, len(keys) + 1)))
    if extra:
        problems.append(f"numbers past the last text: {extra}")
    if problems:
        sys.exit("\n".join(problems))
    arr = lambda name: "[" + ", ".join(rust(v) for v in header[name]) + "]"
    lines = [
        f"//! {module}: every text the app wraps in `tr` or `trf`, keyed by the English.",
        "//! Written by tools/i18n_gen.py.",
        "",
        "pub const TABLE: super::Table = super::Table {",
        f"    months: {arr('months')},",
        f"    months_full: {arr('months_full')},",
        f"    days: {arr('days')},",
        f"    days_full: {arr('days_full')},",
        "    text: TEXT,",
        "};",
        "",
        "pub const TEXT: &[(&str, &str)] = &[",
    ]
    lines += [f"    ({rust(en)}, {rust(text[i])})," for i, en in enumerate(keys, 1)]
    lines += ["];", ""]
    (I18N / f"{module}.rs").write_text("\n".join(lines), encoding="utf-8", newline="\n")
    print(f"{module}.rs: {len(keys)} texts")


def main():
    if sys.argv[1] == "keys":
        for i, en in enumerate(english_keys(), 1):
            print(f"{i}\t{en.replace(chr(10), chr(92) + 'n')}")
    elif sys.argv[1] == "build":
        module, tsv, rule = sys.argv[2], sys.argv[3], sys.argv[4]
        build(module, tsv, FORMS[rule])


if __name__ == "__main__":
    main()
