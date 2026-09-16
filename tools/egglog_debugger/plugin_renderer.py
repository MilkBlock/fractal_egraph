"""Adapter to the actual Eggplant extension; do not copy its rendering algorithms."""
import functools
import json
from pathlib import Path
import subprocess


class PluginRenderer:
    def __init__(self, root, extractor=None):
        self.root = Path(root).resolve()
        self.extractor = extractor
        self.script = Path(__file__).with_name('plugin-renderer.cjs')
        if not (self.root / 'out/dot.js').is_file():
            raise FileNotFoundError(f'Compile the Eggplant extension first: cd {self.root} && npm run compile')
        # Consume the very same webview helpers instead of maintaining a second
        # formula overlay implementation. Fail visibly if the plugin changes shape.
        source = (self.root / 'out/previewPanel.js').read_text()
        helpers = []
        for name, following in [
            ('parseSvgDimension', 'const constraintHighlightColor'),
            ('encodeSvgDataUri', 'const findNodeShape'),
            ('findNodeShape', 'const clearConstraintHighlightArtifacts'),
            ('applyTypstRenderings', 'const applyConstraintCountBadges'),
        ]:
            start = source.index(f'const {name} = ')
            end = source.index(following, start)
            helpers.append(source[start:end].strip())
        self.overlay = ('\n'.join(helpers) + '\nexport { applyTypstRenderings };\n').encode()

    @functools.lru_cache(maxsize=64)
    def _render(self, request):
        command = ['node', str(self.script), str(self.root)]
        if self.extractor:
            command.append(str(self.extractor))
        result = subprocess.run(command, input=request, capture_output=True, text=True, timeout=45)
        if result.returncode:
            raise ValueError(result.stderr[-12000:] or 'Eggplant plugin renderer failed')
        # Cache the serialized output; each caller receives its own immutable snapshot.
        json.loads(result.stdout)
        return result.stdout.encode()

    def render(self, request):
        fields = {key: request[key] for key in ['source', 'line', 'mode', 'label_style', 'recursive_strategy'] if key in request}
        return self._render(json.dumps(fields, ensure_ascii=False, sort_keys=True))


def default_plugin_root(repo):
    # Match the user's installed VS Code extension, including its .egg annotation
    # support, rather than assuming the sibling development checkout is identical.
    installed = list((Path.home() / '.vscode/extensions').glob('milkblock.eggplant-pattern-vscode-*'))
    if installed:
        def version(folder):
            return tuple(int(p) for p in json.loads((folder / 'package.json').read_text())['version'].split('.') if p.isdigit())
        return max(installed, key=version)
    return repo.parent / 'eggplant_pattern_view_plugin/eggplant-pattern-vscode'
