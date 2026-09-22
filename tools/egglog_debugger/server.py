#!/usr/bin/env python3
"""Same-origin local bridge from egglog-demo to the actual egg_layout runtime."""
import os
import argparse
import functools
import importlib.util
import sys
import json
import re
import uuid
from urllib.parse import urlsplit
from pathlib import Path
import shutil
import select
import socket
import threading
import time
import subprocess
import tempfile
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from plugin_renderer import PluginRenderer, default_plugin_root

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_DEMO = ROOT.parent / 'egglog-demo'
# Every web run writes a fresh out/debugger/<uuid>/ tree, and a single round snapshot can
# reach tens of MB (round JSON embeds the whole analysis). Nothing ever removed them, so the
# directory grew to 14 GB and a run failed with ENOSPC. Keep a bounded number of the most
# recent runs. Only directories that look like our own run ids are ever touched.
RUN_ROOT = ROOT / 'out' / 'debugger'
RUN_LIMIT = max(1, int(os.environ.get('EGG_LAYOUT_DEBUGGER_RUNS', '3')))


def prune_runs(keep=RUN_LIMIT):
    """Delete the oldest run directories beyond `keep`. Best effort, never raises.

    Callers must run this before creating a new run folder, so the new (not yet existing)
    directory is never a candidate. A page still displaying a pruned run will 404 on
    further round fetches; keeping the newest few is what makes that unlikely.
    """
    try:
        if not RUN_ROOT.is_dir():
            return
        runs = sorted((p for p in RUN_ROOT.iterdir()
                       if p.is_dir() and re.fullmatch(r'[0-9a-f]{32}', p.name)),
                      key=lambda p: p.stat().st_mtime, reverse=True)
        for stale in runs[keep:]:
            shutil.rmtree(stale, ignore_errors=True)
    except OSError:
        pass


# The example list is a build artifact of ../egglog-demo, which tracks upstream egglog main.
# This repository pins a pristine egg-smol baseline plus local instrumentation, so several
# upstream examples (scheduler DSL, Rational) do not even parse here. Merge in the
# repository's own fixtures, which are known to run on this kernel and, importantly, to
# produce shared SaturatedRuleComposition states.
LOCAL_EXAMPLES = ROOT / 'tools' / 'egglog_debugger' / 'local-examples.json'
# Written by classify_examples.py: which examples the pinned kernel can actually parse.
EXAMPLE_SUPPORT = ROOT / 'tools' / 'egglog_debugger' / 'example-support.json'
# Written by verify_examples.py: which examples a real run showed to produce shared states.
EXAMPLE_VERIFIED = ROOT / 'tools' / 'egglog_debugger' / 'example-verified.json'


def merged_examples(demo: Path, supported_only: bool = True):
    examples = {}
    for base in (demo / 'static', demo / 'dist'):
        path = base / 'examples.json'
        if path.is_file():
            examples = json.loads(path.read_text())
            break
    try:
        local = json.loads(LOCAL_EXAMPLES.read_text())
    except (OSError, ValueError):
        local = {}
    for name, relative in local.items():
        source = ROOT / relative
        if source.is_file():
            examples[name] = source.read_text()
    if not supported_only:
        return examples
    # Preferred filter: only what a real run showed to produce shared SaturatedRuleComposition
    # states. Parsing alone is not enough -- the upstream egg math demo parses and saturates
    # nothing, and so do fibonacci/list/path/set/unify/naturals on this kernel. Offering entries
    # that cannot reach the panel being studied is worse than a short list.
    try:
        verified = {e['name'] for e in json.loads(EXAMPLE_VERIFIED.read_text()).get('verified') or []}
    except (OSError, ValueError, TypeError, KeyError):
        verified = set()
    if verified:
        verified |= set(local)  # the repository's own fixtures are verified by construction
        return {name: source for name, source in examples.items() if name in verified}
    # No sweep recorded yet: fall back to what the kernel can at least parse.
    try:
        allowed = set(json.loads(EXAMPLE_SUPPORT.read_text()).get('supported') or [])
    except (OSError, ValueError):
        return examples
    if not allowed:
        return examples
    allowed |= set(local)
    return {name: source for name, source in examples.items() if name in allowed}


class Handler(SimpleHTTPRequestHandler):
    def do_GET(self):
        match = re.fullmatch(r'/api/runs/([0-9a-f]{32})/rounds/(round-[0-9]+\.(?:layers\.dot|fractals\.dot|coverage\.dot|reuse\.dot|use_fractals\.dot|saturated_rule_composition\.dot|json)|manifest\.json)', urlsplit(self.path).path)
        if match:
            file = ROOT / 'out' / 'debugger' / match[1] / 'rounds' / match[2]
            if not file.is_file():
                return self.reply(404, {'error':'Unknown run artifact'})
            return self.reply(200,file.read_bytes(),'text/vnd.graphviz; charset=utf-8' if file.suffix=='.dot' else 'application/json')
        if self.path.split('?')[0] == '/plugin-overlay.js':
            return self.reply(200, self.server.renderer.overlay, 'text/javascript')
        if self.path.split('?')[0] == '/examples.json':
            # ?all=1 lists everything the demo ships, including what this kernel cannot parse.
            every = 'all=1' in self.path
            return self.reply(200, merged_examples(self.server.demo, not every))
        return super().do_GET()

    def do_OPTIONS(self):
        return self.reply(204, b'')

    def translate_path(self, path):
        if path.split('?')[0] in ('/native-debugger.js', '/native-debugger.css', '/wasm-worker.js', '/layer-panel.js', '/ripen-coverage.mjs'):
            return str(ROOT / 'tools/egglog_debugger' / path.split('?')[0][1:])
        # The browser bundle is a build artifact (see browser/build.mjs); the local
        # page imports it to generate a fractal lane's `.egg`, so serve it when it
        # was built rather than 404ing every fractal preview.
        if path.split('?')[0].startswith('/browser/'):
            return str(ROOT / 'target/pages' / path.split('?')[0][1:])
        # Source static files take precedence over an old dist build.
        clean = path.split('?')[0].lstrip('/') or 'index.html'
        candidate = (self.server.demo / 'static' / clean).resolve()
        if candidate.is_relative_to(self.server.demo / 'static') and candidate.is_file():
            return str(candidate)
        return super().translate_path(path)

    def allowed_origin(self):
        """Same-origin plus the configured static-deployment origins."""
        origin = self.headers.get('Origin')
        if not origin:
            return None
        if origin == 'http://' + self.headers.get('Host', ''):
            return origin
        return origin if origin in self.server.allowed_origins else False

    def reply(self, code, data, content_type='application/json'):
        body = data if isinstance(data, bytes) else json.dumps(data).encode()
        origin = self.allowed_origin() if hasattr(self.server, 'allowed_origins') else None
        self.send_response(code)
        self.send_header('Content-Type', content_type)
        self.send_header('Content-Length', str(len(body)))
        self.send_header('Cache-Control', 'no-store')
        if origin:
            self.send_header('Access-Control-Allow-Origin', origin)
            self.send_header('Access-Control-Allow-Headers', 'Content-Type')
            self.send_header('Access-Control-Allow-Methods', 'GET, POST, OPTIONS')
            self.send_header('Vary', 'Origin')
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        streaming = False
        # Same-origin, or an origin explicitly listed with --allow-origin (a static
        # deployment of the UI may connect to a bridge running on this machine).
        origin = self.allowed_origin()
        if origin is False:
            return self.reply(403, {'error': 'Cross-origin request refused'})
        if self.headers.get('Content-Type', '').split(';')[0] != 'application/json':
            return self.reply(415, {'error': 'Expected application/json'})
        try:
            length = int(self.headers.get('Content-Length', '0'))
            if not 0 < length <= 2_000_000:
                return self.reply(413, {'error': 'Source must be at most 2 MB'})
            data = json.loads(self.rfile.read(length))
            if self.path == '/api/saturated-rule-composition-catalog':
                folder = (ROOT / data['path']).resolve()
                if not folder.is_relative_to(ROOT / 'out'):
                    return self.reply(400, {'error':'请选择仓库 out/ 内的目录'})
                if not (folder / 'catalog.json').is_file() and (folder / 'catalog/catalog.json').is_file():
                    folder = (folder / 'catalog').resolve()
                if not folder.is_relative_to(ROOT / 'out'):
                    raise ValueError('Catalog must remain inside out/')
                if not (folder / 'catalog.json').is_file():
                    return self.reply(404, {'error': f'{data["path"]} 中没有 catalog.json：请选择一次带 SaturatedRuleComposition 的运行目录（含 catalog/），或 out/saturated-rule-composition-* 目录'})
                catalog = json.loads((folder / 'catalog.json').read_text())
                if catalog.get('schema') != 'saturated-rule-composition-catalog/v1':
                    raise ValueError('Unsupported saturated-rule-composition catalog')
                states = [json.loads((folder / 'states' / f'state-{i:04}.json').read_text()) for i in range(catalog['saturated_rule_compositions'])]
                queue_file = folder.parent / 'saturated-rule-composition/queue.json'
                coverage_context = None
                if queue_file.is_file():
                    queue = json.loads(queue_file.read_text())
                    if (queue.get('catalog') or {}).get('catalog', {}).get('triggers') == catalog.get('triggers'):
                        coverage_context = {'cs': queue.get('cs'), 'counts': queue.get('counts')}
                return self.reply(200, {'catalog':catalog, 'dot':(folder / 'catalog.dot').read_text(), 'states':states, 'coverage_context':coverage_context})
            if self.path == '/api/saturated-rule-composition-body':
                # The body of one state: the native egglog graphs written by the ripen cell,
                # plus its entry program. Never reconstructed -- these are the files ripen wrote.
                folder = (ROOT / data['path']).resolve()
                if not folder.is_relative_to(ROOT / 'out'):
                    return self.reply(400, {'error':'请选择仓库 out/ 内的目录'})
                if not (folder / 'catalog.json').is_file() and (folder / 'catalog/catalog.json').is_file():
                    folder = (folder / 'catalog').resolve()
                if not folder.is_relative_to(ROOT / 'out'):
                    raise ValueError('Catalog must remain inside out/')
                catalog = json.loads((folder / 'catalog.json').read_text())
                state = int(data['state'])
                trigger = next((t for t in catalog.get('triggers') or []
                                if t.get('saturated_rule_composition') == state), None)
                if trigger is None:
                    return self.reply(404, {'error': f'C{state} 没有触发实例，无法定位 cell'})
                cell = Path(trigger['source']).resolve()
                if not cell.is_relative_to(ROOT / 'out'):
                    raise ValueError('Cell must remain inside out/')
                body = {'state':state, 'cell':str(cell), 'entry':None}
                for key, name in (('entry','entry.egg'),('dot','native-egraph.dot'),
                                  ('initial_dot','native-egraph-initial.dot'),
                                  ('svg','native-egraph.svg'),
                                  ('initial_svg','native-egraph-initial.svg')):
                    path = cell / name
                    body[key] = path.read_text() if path.is_file() else None
                return self.reply(200, body)
            if self.path == '/api/layer-run':
                folder = (ROOT / data['path']).resolve()
                if not folder.is_relative_to(ROOT / 'out'):
                    return self.reply(400, {'error':'请选择仓库 out/ 内的分析目录'})
                manifest = json.loads((folder / 'rounds/manifest.json').read_text())
                frames = []
                for item in manifest:
                    stem = item['stem']
                    if not re.fullmatch(r'round-[0-9]+',stem):
                        raise ValueError('Invalid snapshot filename')
                    frame = json.loads((folder / 'rounds' / (stem+'.json')).read_text())
                    if frame.get('kind') != 'layer_snapshot':
                        raise ValueError('旧快照没有渲染数据，请使用当前版本重新 analyze/replay')
                    frames.append(frame)
                catalog_folder = (folder / 'catalog').resolve()
                saturated_rule_composition_catalog = str(catalog_folder.relative_to(ROOT)) if catalog_folder.is_relative_to(ROOT / 'out') and (catalog_folder / 'catalog.json').is_file() else None
                return self.reply(200, {'frames':frames, 'saturated_rule_composition_catalog':saturated_rule_composition_catalog})
            if self.path == '/api/preview':
                line = data.get('line', 1)
                data['edit_targets'] = self.server.annotations.catalog(data['source'], line)
                data['rule_entry'] = self.server.annotations.rewrite_preview_entry(data['source'], line)
                # `birewrite` has no add_rule scope; preview its forward rewrite instead.
                data['preview_source'] = self.server.annotations.preview_source(data['source'], line)
                return self.reply(200, self.server.renderer.render(data))
            if self.path == '/api/edit-conditions':
                result = self.server.annotations.update_conditions(data['source'], data['line'], data['conditions'])
                with tempfile.TemporaryDirectory(prefix='egg-condition-check-') as folder:
                    source = Path(folder) / 'input.egg'
                    source.write_text(result['source'])
                    check = subprocess.run([str(self.server.binary), 'debug-patterns', str(source)], capture_output=True, timeout=30, cwd=ROOT)
                    if check.returncode:
                        return self.reply(422, {'error': check.stderr.decode(errors='replace')})
                return self.reply(200, result)
            if self.path == '/api/edit-display':
                result = self.server.annotations.update_display(
                    data['source'], data['line'], data['target_id'], data['value'],
                    data.get('fields'), data.get('template'), data.get('precedence'))
                if result.get('template'):
                    check = self.server.renderer.validate_template(result['template'], result.get('template_fields') or [])
                    if not check.get('ok'):
                        error = 'Typst 模板无法编译：' + check.get('error', '未知错误')
                        # A repairable template stays unwritten until the user confirms
                        # the suggestion on a second submit.
                        if check.get('suggestion') and check['suggestion'] != result['template']:
                            return self.reply(409, {'error': error, 'suggestion': check['suggestion'], 'notes': check.get('notes') or []})
                        return self.reply(422, {'error': error})
                return self.reply(200, result)
            if self.path in ('/api/patterns', '/api/trace') and not (
                    self.server.binary.is_file() and os.access(self.server.binary, os.X_OK)):
                return self.reply(503, {'error': f'egglog 运行程序不存在或不可执行：{self.server.binary}。请重新启动本地服务（不指定 --binary 会自动构建）。'})
            with tempfile.TemporaryDirectory(prefix='egglog-debug-') as folder:
                folder = Path(folder)
                if self.path == '/api/render':
                    kind = data['kind']
                    if kind not in ('typst', 'dot'):
                        return self.reply(400, {'error': 'Unknown renderer'})
                    src = folder / ('formula.typ' if kind == 'typst' else 'graph.dot')
                    src.write_text(data['source'])
                    dest = folder / 'preview.svg'
                    command = (['typst', 'compile', '--root', str(folder), str(src), str(dest)]
                               if kind == 'typst' else ['dot', '-Tsvg', str(src), '-o', str(dest)])
                    result = subprocess.run(command, capture_output=True, timeout=20)
                    if result.returncode:
                        return self.reply(422, {'error': result.stderr.decode(errors='replace')})
                    return self.reply(200, dest.read_bytes(), 'image/svg+xml')
                if self.path not in ('/api/patterns', '/api/trace'):
                    return self.reply(404, {'error': 'Unknown endpoint'})
                src = folder / 'input.egg'
                src.write_text(data['source'])
                command = [str(self.server.binary), 'debug-patterns' if self.path == '/api/patterns' else 'debug-stream', str(src)]
                if self.path == '/api/patterns':
                    result = subprocess.run(command, capture_output=True, timeout=30, cwd=ROOT)
                    if result.returncode:
                        return self.reply(422, {'error': result.stderr.decode(errors='replace')})
                    return self.reply(200, result.stdout)
                run_id = uuid.uuid4().hex
                run_folder = RUN_ROOT / run_id
                (run_folder / 'rounds').mkdir(parents=True)
                # Prune *after* the new folder exists, so RUN_LIMIT counts it. Pruning first
                # would keep RUN_LIMIT stale runs and then add one more.
                prune_runs()
                (run_folder / 'source.egg').write_text(data['source'])
                snapshots = []
                self.send_response(200)
                self.send_header('Content-Type', 'application/x-ndjson')
                self.send_header('Cache-Control', 'no-store')
                # The streamed response is written by hand, so it needs its own CORS headers.
                if origin:
                    self.send_header('Access-Control-Allow-Origin', origin)
                    self.send_header('Access-Control-Allow-Headers', 'Content-Type')
                    self.send_header('Access-Control-Allow-Methods', 'GET, POST, OPTIONS')
                    self.send_header('Vary', 'Origin')
                self.end_headers()
                streaming = True
                with (folder / 'stderr').open('w+') as errors:
                    env = dict(os.environ, EGG_LAYOUT_SATURATED_RULE_COMPOSITION_OUTPUT=str(run_folder))
                    process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=errors, cwd=ROOT, env=env)
                    finished = threading.Event()
                    timed_out = threading.Event()
                    def watch():
                        deadline = time.monotonic() + self.server.run_timeout
                        while not finished.wait(0.2):
                            if time.monotonic() >= deadline:
                                timed_out.set()
                                process.kill()
                                return
                            try:
                                readable, _, _ = select.select([self.connection], [], [], 0)
                                if readable and not self.connection.recv(1, socket.MSG_PEEK):
                                    process.kill()
                                    return
                            except OSError:
                                if process.poll() is None:
                                    process.kill()
                                return
                    watcher = threading.Thread(target=watch, daemon=True)
                    watcher.start()
                    try:
                        for line in process.stdout:
                            try:
                                row = json.loads(line)
                            except (ValueError, UnicodeDecodeError):
                                row = {}
                            if row.get('kind') == 'layer_snapshot':
                                stem = f"round-{len(snapshots)+1:04}"
                                row['artifact_base'] = f'/api/runs/{run_id}/rounds/'
                                row['artifact_directory'] = str(run_folder.relative_to(ROOT))
                                row['stem'] = stem
                                for kind in ('layers','fractals','coverage','reuse','use_fractals','saturated_rule_composition'):
                                    if kind not in row['dots']: continue
                                    (run_folder / 'rounds' / f'{stem}.{kind}.dot').write_text(row['dots'][kind])
                                (run_folder / 'rounds' / f'{stem}.json').write_text(json.dumps(row,ensure_ascii=False))
                                snapshots.append({key:row[key] for key in ('label','round','end','stem','counts')})
                                (run_folder / 'rounds/manifest.json').write_text(json.dumps(snapshots,ensure_ascii=False))
                                line = (json.dumps(row,ensure_ascii=False)+'\n').encode()
                            self.wfile.write(line)
                            self.wfile.flush()
                        if process.wait():
                            errors.seek(0)
                            self.wfile.write((json.dumps({'kind': 'error', 'error': 'Run time limit exceeded' if timed_out.is_set() else errors.read()[-12000:]})+'\n').encode())
                    except (BrokenPipeError, ConnectionResetError):
                        process.terminate()
                    finally:
                        finished.set()
                        if process.poll() is None:
                            process.terminate()
                        process.wait()
                self.close_connection = True
        except (KeyError, ValueError, subprocess.TimeoutExpired, OSError) as error:
            if streaming:
                # Headers are already sent. A second HTTP response corrupts NDJSON.
                self.close_connection = True
                try:
                    self.wfile.write((json.dumps({'kind': 'error', 'error': str(error)})+'\n').encode())
                    self.wfile.flush()
                except OSError:
                    pass  # The client may have cancelled the stream.
            else:
                self.reply(400, {'error': str(error)})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--demo', type=Path, default=DEFAULT_DEMO)
    parser.add_argument('--port', type=int, default=8080)
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--plugin', type=Path, default=default_plugin_root(ROOT))
    parser.add_argument('--extractor', type=Path, help='Use the same extractor override as VS Code')
    parser.add_argument('--run-timeout', type=float, default=120, help='Native run limit in seconds')
    parser.add_argument('--allow-origin', action='append', default=None,
                        help='Extra Origin allowed to call this bridge (repeatable); the static UI '
                             'deployment uses it to reach a bridge running on your machine')
    args = parser.parse_args()
    if args.binary is None:
        subprocess.run(['cargo', 'build', '--release', '--bin', 'egg_layout'], cwd=ROOT, check=True)
        metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--no-deps', '--format-version=1'], cwd=ROOT))
        args.binary = Path(metadata['target_directory']) / 'release/egg_layout'
    if not args.binary.is_file() or not os.access(args.binary, os.X_OK):
        parser.error(f'Runtime binary is missing or not executable: {args.binary}; omit --binary to build automatically')
    for tool in ('typst', 'dot'):
        if not shutil.which(tool):
            parser.error(f'{tool} must be installed to render previews')
    server = ThreadingHTTPServer(('127.0.0.1', args.port), functools.partial(Handler, directory=str(args.demo.resolve() / 'dist')))
    server.renderer = PluginRenderer(args.plugin, args.extractor)
    server.demo = args.demo.resolve()
    spec = importlib.util.spec_from_file_location('demo_preview_annotations', server.demo / 'preview_annotations.py')
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    server.annotations = module
    server.binary = args.binary.resolve()
    server.run_timeout = args.run_timeout
    server.allowed_origins = set(args.allow_origin or []) | {'https://milkblock.github.io'}
    print(f'Native egglog debugger: http://127.0.0.1:{args.port}', flush=True)
    server.serve_forever()

if __name__ == '__main__':
    main()
