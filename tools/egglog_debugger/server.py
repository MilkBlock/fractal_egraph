#!/usr/bin/env python3
"""Same-origin local bridge from egglog-demo to the actual egg_layout runtime."""
import argparse
import functools
import importlib.util
import sys
import json
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

class Handler(SimpleHTTPRequestHandler):
    def do_GET(self):
        if self.path.split('?')[0] == '/plugin-overlay.js':
            return self.reply(200, self.server.renderer.overlay, 'text/javascript')
        return super().do_GET()

    def do_OPTIONS(self):
        return self.reply(204, b'')

    def translate_path(self, path):
        if path.split('?')[0] in ('/native-debugger.js', '/native-debugger.css'):
            return str(ROOT / 'tools/egglog_debugger' / path.split('?')[0][1:])
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
                with (folder / 'stderr').open('w+') as errors:
                    process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=errors, cwd=ROOT)
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
        except (KeyError, ValueError, subprocess.TimeoutExpired, FileNotFoundError) as error:
            self.reply(400, {'error': str(error)})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--demo', type=Path, default=ROOT.parent / 'egglog-demo')
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
