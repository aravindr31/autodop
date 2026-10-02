#!/usr/bin/env python3
"""Strip every comment from source files without touching a single code line.

Language-aware, because comments hide inside strings and strings hide inside
comments:
- C-like (Rust, TS/TSX, JS): // and /* */ line/block comments; ", ', ` strings
  (with ${} interpolation nesting), Rust lifetimes ('a) vs char literals ('x'),
  raw strings (r#"…"#), byte strings.
- YAML: # comments outside quoted scalars.
Whole-line comments remove the line; trailing comments trim to the code.

Docstrings in Python are statements, not comments, and the legacy Python files
are left untouched by policy — this tool only rewrites app sources.
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

def strip_clike(src: str, rust: bool = False) -> str:
    out = []
    i, n = 0, len(src)
    # `removed` counts consecutive removed comment lines so blank-line collapse
    # can drop the leftovers without touching code positions.
    stack = []  # 'tpl' for template literals, with brace depth
    in_string = None  # '"', "'", 'raw', ('tpl', depth)
    brace_depth_tpl = 0

    def emit(ch):
        out.append(ch)

    while i < n:
        c = src[i]
        nxt = src[i + 1] if i + 1 < n else ''

        # --- inside a template literal? ---
        if stack and stack[-1][0] == 'tpl':
            if c == '`':
                emit(c); stack.pop(); i += 1; continue
            if c == '\\' and i + 1 < n:
                emit(c); emit(src[i+1]); i += 2; continue
            if c == '$' and nxt == '{':
                emit(c); emit(nxt)
                stack.append(['expr', 1])
                i += 2; continue
            emit(c); i += 1; continue

        # --- inside a template ${} expression? ---
        if stack and stack[-1][0] == 'expr':
            if c == '{':
                stack[-1][1] += 1; emit(c); i += 1; continue
            if c == '}':
                stack[-1][1] -= 1
                emit(c); i += 1
                if stack[-1][1] == 0:
                    stack.pop()  # back into template text
                continue
            # strings inside the expression are handled by the generic states
            # below (fall through).

        # --- line comments ---
        if in_string is None and c == '/' and nxt == '/':
            # swallow to end of line
            while i < n and src[i] != '\n':
                i += 1
            continue
        # --- block comments ---
        if in_string is None and c == '/' and nxt == '*':
            j = src.find('*/', i + 2)
            end = n if j == -1 else j + 2
            if '\n' in src[i:end]:
                # multi-line: keep the newlines to preserve line structure of code
                for ch in src[i:end]:
                    if ch == '\n':
                        emit(ch)
                    else:
                        pass
            else:
                emit(' ')  # single-line: keep one separator so code doesn't fuse
            i = end
            continue

        # --- raw strings (Rust r"…" / r#"…"#) ---
        if in_string is None and c == 'r' and (nxt == '"' or nxt == '#'):
            hashes = 0
            k = i + 1
            while k < n and src[k] == '#':
                hashes += 1; k += 1
            if k < n and src[k] == '"':
                emit(src[i:k+1])
                closer = '"' + '#' * hashes
                j = src.find(closer, k + 1)
                j = n if j == -1 else j + len(closer)
                out.append(src[k+1:j])
                i = j
                continue
            emit(c); i += 1; continue

        # --- normal strings ---
        if in_string is None and c in '"\'':
            # Rust lifetime vs char literal: 'x' / '\n' are literals; 'a alone is not.
            # Applies to Rust only — in JS every ' opens a string.
            if rust and c == "'" and not (
                (nxt == '\\' and i + 3 < n and src[i+3] == "'")
                or (i + 2 < n and src[i+2] == "'")
            ):
                emit(c); i += 1; continue
            in_string = c
            emit(c); i += 1; continue
        if in_string in ('"', "'") and c == in_string:
            in_string = None
            emit(c); i += 1; continue
        if in_string and c == '\\' and i + 1 < n:
            emit(c); emit(src[i+1]); i += 2; continue

        # --- template literal start (JS) ---
        if in_string is None and c == '`':
            emit(c)
            stack.append(['tpl', 0])
            i += 1; continue

        emit(c); i += 1

    text = ''.join(out)
    return text

def tidy(text: str) -> str:
    """Drop lines that are now blank *and* were comment-only, keep code lines."""
    lines = text.split('\n')
    result = []
    for line in lines:
        if line.strip() == '':
            result.append(None)  # candidate for removal, keep for now
        else:
            result.append(line.rstrip())
    # Collapse runs of blank lines that comment removal created: a blank line
    # stays only if it is surrounded by code lines both above and below.
    out = []
    for idx, line in enumerate(result):
        if line is None:
            prev_code = any(l is not None for l in out_reversed(out))
            next_code = any(l is not None for l in result[idx+1:] if l is not None)
            if prev_code and next_code:
                out.append('')
            continue
        out.append(line)
    # trim leading/trailing blank lines
    while out and out[0] == '':
        out.pop(0)
    while out and out[-1] == '':
        out.pop()
    return '\n'.join(out) + '\n'

def out_reversed(out):
    return reversed(out)

def strip_yaml(src: str) -> str:
    out = []
    for line in src.split('\n'):
        stripped = line
        # remove # comments outside quotes
        quote = None
        i = 0
        cut = None
        while i < len(stripped):
            c = stripped[i]
            if quote:
                if c == quote:
                    quote = None
            elif c in ('"', "'"):
                quote = c
            elif c == '#' and (i == 0 or stripped[i-1] in ' \t'):
                cut = i
                break
            i += 1
        if cut is not None:
            stripped = stripped[:cut].rstrip()
        if stripped == '' and line.strip() != '':
            continue  # comment-only line → drop
        out.append(stripped.rstrip() if stripped.strip() == '' else stripped)
    text = '\n'.join(out)
    while '\n\n\n' in text:
        text = text.replace('\n\n\n', '\n\n')
    return text.rstrip() + '\n' if text.strip() else text

def strip_python(src: str) -> str:
    out = []
    i, n = 0, len(src)
    while i < n:
        c = src[i]
        # triple-quoted strings (docstrings) — keep verbatim
        if src[i:i+3] in ('"""', "'''"):
            q = src[i:i+3]
            j = src.find(q, i + 3)
            j = n if j == -1 else j + 3
            out.append(src[i:j]); i = j; continue
        if c == '#':
            while i < n and src[i] != '\n':
                i += 1
            continue
        if c in ('"', "'"):
            j = i + 1
            while j < n:
                if src[j] == '\\':
                    j += 2; continue
                if src[j] == c:
                    j += 1; break
                j += 1
            out.append(src[i:j]); i = j; continue
        out.append(c); i += 1
    text = ''.join(out)
    # drop comment-only lines
    lines = [ln for ln in text.split('\n')]
    result = []
    for ln in lines:
        if ln.strip() == '':
            continue
        result.append(ln.rstrip())
    # keep original blank lines that separated code
    return '\n'.join(result) + '\n'

def process(path: Path) -> None:
    src = path.read_text()
    suffix = path.suffix
    if suffix in ('.rs', '.ts', '.tsx', '.mjs', '.js'):
        new = tidy(strip_clike(src, rust=suffix == '.rs'))
    elif suffix == '.yml' or suffix == '.yaml':
        new = strip_yaml(src)
    else:
        new = strip_python(src)
    if new != src:
        path.write_text(new)
        print(f'stripped {path.relative_to(ROOT)}')

patterns = [
    'src-tauri/src/*.rs',
    'src-tauri/src/**/*.rs',
    'src-tauri/examples/*.rs',
    'src-tauri/build.rs',
    'frontend/src/**/*.ts',
    'frontend/src/**/*.tsx',
    'frontend/src/**/*.astro',
    'scripts/*.mjs',
    'frontend/astro.config.mjs',
    'frontend/scripts/*.mjs',
    '.github/workflows/*.yml',
]
count = 0
for pattern in patterns:
    for path in ROOT.glob(pattern):
        if path.is_file():
            process(path)
            count += 1
print(f'{count} files processed')