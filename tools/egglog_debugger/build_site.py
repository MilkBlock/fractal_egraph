#!/usr/bin/env python3
"""Assemble the static build of the native debugger UI for GitHub Pages.

GitHub Pages cannot run the bridge (a local process), so the published page runs the
patched runtime and the whole preview pipeline from wasm: the instrumented egglog
runtime, the Eggplant transpiler and extractor, Typst and Graphviz. Editing that writes
back to .egg still uses a bridge on the visitor's machine when one is running.
"""
import argparse
import os
import shutil
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from plugin_renderer import PluginRenderer, default_plugin_root

ROOT = Path(__file__).resolve().parents[2]
DEBUGGER = ROOT / 'tools/egglog_debugger'
WASM = DEBUGGER / 'wasm'
BROWSER = DEBUGGER / 'browser'
EXTRACTOR_WASM = ROOT / 'target/extractor-wasm'


def build_browser(out, plugin, webdeps=None, extractor_wasm=None):
    """Bundle the wasm preview pipeline the page loads when no bridge is running."""
    if not shutil.which('node'):
        raise SystemExit('node is required to bundle the browser preview pipeline')
    command = ['node', str(BROWSER / 'build.mjs'), '--plugin', str(plugin), '--out', str(out)]
    if webdeps:
        command += ['--webdeps', str(webdeps)]
    else:
        # Default to the sibling web editor checkout, the only place the typst.ts
        # and @viz-js/viz dependencies are installed for this build.
        command += ['--webdeps', str(ROOT / 'dpsk_workspace/viz-web-editor/node_modules')]
    if extractor_wasm:
        command += ['--extractor-wasm', str(extractor_wasm)]
    subprocess.run(command, check=True)


def build_wasm(out):
    """Compile the patched instrumented runtime to wasm and bind it for the browser."""
    if not shutil.which('wasm-bindgen'):
        raise SystemExit('wasm-bindgen CLI is required: cargo install wasm-bindgen-cli --version 0.2.128')
    env = {**os.environ, 'RUSTFLAGS': '--cfg getrandom_backend="wasm_js"'}
    subprocess.run(['cargo', 'build', '--release', '--target', 'wasm32-unknown-unknown'], cwd=WASM, env=env, check=True)
    artifact = WASM / 'target/wasm32-unknown-unknown/release/egglog_debug_wasm.wasm'
    subprocess.run(['wasm-bindgen', str(artifact), '--target', 'web', '--no-typescript',
                    '--out-dir', str(out / 'wasm')], check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--demo', type=Path, default=ROOT.parent / 'egglog-demo')
    parser.add_argument('--plugin', type=Path, default=default_plugin_root(ROOT))
    parser.add_argument('--extractor', type=Path, help='Same extractor override as VS Code')
    parser.add_argument('--webdeps', type=Path, help='node_modules holding typst.ts and @viz-js/viz')
    parser.add_argument('--extractor-wasm', type=Path, default=EXTRACTOR_WASM,
                        help='Prebuilt wasm-pack output for the extractor crate')
    parser.add_argument('--skip-browser', action='store_true', help='Only build the runtime wasm')
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
    for name in ('native-debugger.js', 'native-debugger.css', 'wasm-worker.js', 'layer-panel.js', 'ripen-coverage.mjs'):
        shutil.copy2(DEBUGGER / name, out / name)
    build_wasm(out)
    if not args.skip_browser:
        extractor_wasm = args.extractor_wasm if Path(args.extractor_wasm).is_dir() else None
        build_browser(out, args.plugin, args.webdeps, extractor_wasm)
    # The bridge serves this overlay from the installed plugin; bake it for the static page.
    (out / 'plugin-overlay.js').write_bytes(PluginRenderer(args.plugin, args.extractor).overlay)
    (out / '.nojekyll').write_text('')
    total = sum(path.stat().st_size for path in out.rglob('*') if path.is_file())
    print(f'{out} ({total / 1_000_000:.1f} MB, {len(list(out.iterdir()))} entries)')
    return out


if __name__ == '__main__':
    main()
