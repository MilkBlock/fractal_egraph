"""Keep mathematical identifiers in subtitles; spell them explicitly for speech."""
import re

READINGS = {
    **{f'{letter}{i}': f'字母 {letter.upper()}，{digit}'
       for letter in ('a', 'b') for i, digit in enumerate(('零', '一', '二'))},
    'a': '字母 A', 'b': '字母 B', 'A': '字母 A', 'B': '字母 B',
    't': '字母 T', 'Tiger': '泰格',
}
TOKENS = re.compile(r'(?<![A-Za-z0-9_])(?:' + '|'.join(
    re.escape(key) for key in sorted(READINGS, key=len, reverse=True)
) + r')(?![A-Za-z0-9_])')


def spoken_text(subtitle):
    return TOKENS.sub(lambda match: READINGS[match.group()], subtitle)
