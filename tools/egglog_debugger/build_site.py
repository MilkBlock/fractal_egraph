#!/usr/bin/env python3
"""Assemble the static build of the native debugger UI for GitHub Pages.

GitHub Pages cannot run the bridge (patched egglog runtime, the VS Code extension's
extractor, the Typst CLI), so the published page is static and connects to a bridge on
the visitor's machine for native analysis. The upstream WASM Run needs no server.
"""
import argparse
import shutil
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from plugin_renderer import PluginRenderer, default_plugin_root

ROOT = Path(__file__).resolve().parents[2]
DEBUGGER = ROOT / 'tools/egglog_debugger'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--demo', type=Path, default=ROOT.parent / 'egglog-demo')
    parser.add_argument('--plugin', type=Path, default=default_plugin_root(ROOT))
    parser.add_argument('--extractor', type=Path, help='Same extractor override as VS Code')
    parser.add_argument('--output', type=Path, default=ROOT / 'target/pages')
    args = parser.parse_args()
    demo = args.demo.resolve()
    dist, static = demo / 'dist', demo / 'static'
    if not (dist / 'egglog_demo_bg.wasm').is_file():
        parser.error(f'build the demo first: cd {demo} && make')
    if not (static / 'examples.json').is_file():
        parser.error(f'missing {static / "examples.json"}; run the demo make target')
    out = args.output.resolve()
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)
    # `static/` takes precedence over the older dist build, exactly like the bridge.
    for folder in (dist, static):
        for path in sorted(folder.iterdir()):
            if path.name.startswith('.'):
                continue
            if path.is_dir():
                shutil.copytree(path, out / path.name, dirs_exist_ok=True)
            else:
                shutil.copy2(path, out / path.name)
    for name in ('native-debugger.js', 'native-debugger.css'):
        shutil.copy2(DEBUGGER / name, out / name)
    # The bridge serves this overlay from the installed plugin; bake it for the static page.
    (out / 'plugin-overlay.js').write_bytes(PluginRenderer(args.plugin, args.extractor).overlay)
    (out / '.nojekyll').write_text('')
    total = sum(path.stat().st_size for path in out.rglob('*') if path.is_file())
    print(f'{out} ({total / 1_000_000:.1f} MB, {len(list(out.iterdir()))} entries)')
    return out


if __name__ == '__main__':
    main()
