"""One Python token palette for the Manim movie and offline trace reader."""
import builtins
import html
import io
import keyword
import tokenize


PALETTE = {
    'text': '#ECECF4', 'keyword': '#C792EA', 'function': '#82AAFF',
    'builtin': '#89DDFF', 'number': '#F6C177', 'string': '#A3D977',
    'operator': '#89DDFF', 'comment': '#8B93A7',
}


def python_lines(source):
    """Return lossless colored spans; never execute the source being displayed."""
    lines = source.splitlines()
    styles = [['text'] * len(line) for line in lines]
    tokens = []
    try:
        tokens.extend(tokenize.generate_tokens(io.StringIO(source+'\n').readline))
    except (tokenize.TokenError, IndentationError):
        # An explanatory fragment may end inside a block; retain parsed tokens.
        pass
    for i, tok in enumerate(tokens):
        kind = 'text'
        if tok.type == tokenize.NAME:
            if keyword.iskeyword(tok.string): kind = 'keyword'
            elif tok.string in vars(builtins): kind = 'builtin'
            elif i+1 < len(tokens) and tokens[i+1].string == '(': kind = 'function'
        elif tok.type == tokenize.NUMBER: kind = 'number'
        elif tok.type == tokenize.STRING: kind = 'string'
        elif tok.type == tokenize.OP: kind = 'operator'
        elif tok.type == tokenize.COMMENT: kind = 'comment'
        else: continue
        for row in range(tok.start[0]-1, min(tok.end[0], len(lines))):
            start = tok.start[1] if row == tok.start[0]-1 else 0
            end = tok.end[1] if row == tok.end[0]-1 else len(lines[row])
            styles[row][start:end] = [kind]*(end-start)
    result = []
    for line, colors in zip(lines,styles):
        spans = []
        for char, kind in zip(line,colors):
            if spans and spans[-1]['kind'] == kind: spans[-1]['text'] += char
            else: spans.append(dict(text=char,kind=kind))
        result.append(spans)
    return result


def markup_line(spans, trim_start=0):
    """Pango markup, preserving the original geometry of the code block."""
    result = []
    for span in spans:
        text = span['text']
        skipped = min(len(text),trim_start)
        trim_start -= skipped
        text = text[skipped:]
        if text:
            result.append(f'<span foreground="{PALETTE[span["kind"]]}">{html.escape(text)}</span>')
    return ''.join(result) or ' '


def html_code(source):
    return '\n'.join(''.join(f'<span class="syntax-{s["kind"]}">{html.escape(s["text"])}</span>'
                            for s in line) for line in python_lines(source))


def css():
    return ''.join(f'.syntax-{kind}{{color:{color}}}' for kind,color in PALETTE.items())
