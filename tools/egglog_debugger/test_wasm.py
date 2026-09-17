#!/usr/bin/env python3
"""Verify the browser wasm runtime: no bridge, same rows as the native CLI.

Serves the static build (`tools/egglog_debugger/build_site.py --output ...`) and points
the page at a dead bridge, so `运行并识别` must use the wasm instrumented runtime.
"""
import argparse
import functools
import http.server
import threading
from pathlib import Path

from playwright.sync_api import sync_playwright

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / 'experiments/bake/increment-3.egg'


def serve(folder):
    handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=str(folder))
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server, f'http://127.0.0.1:{server.server_port}/'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dir', type=Path, default=ROOT / 'target/pages')
    parser.add_argument('--chromium')
    args = parser.parse_args()
    if not (args.dir / 'wasm/egglog_debug_wasm.js').is_file():
        parser.error(f'missing wasm build in {args.dir}; run tools/egglog_debugger/build_site.py')
    source = SOURCE.read_text()
    server, url = serve(args.dir)
    try:
        with sync_playwright() as p:
            browser = p.chromium.launch(headless=True, **({'executable_path': args.chromium} if args.chromium else {}))
            page = browser.new_page()
            errors = []
            page.on('pageerror', lambda error: errors.append(str(error)))
            # A dead port makes the page take the wasm path even on a loopback host.
            page.add_init_script("window.__EGGLOG_BRIDGE__='http://127.0.0.1:8199'")
            page.goto(url, wait_until='networkidle')
            page.wait_for_selector('#native-run')
            page.wait_for_timeout(1500)
            assert 'wasm' in page.locator('#native-status').inner_text() or '浏览器' in page.locator('#native-status').inner_text()
            page.evaluate('(source)=>{document.querySelector(".CodeMirror").CodeMirror.setValue(source);}', source)
            page.click('#native-run')
            page.wait_for_function('()=>document.querySelector("#native-status").textContent.includes("完成")', timeout=180000)
            status = page.locator('#native-status').inner_text()
            kinds = page.evaluate('()=>{const k={};for(const r of window.egglogNative.rows)k[r.kind]=(k[r.kind]||0)+1;return k;}')
            assert kinds.get('application') == 6 and kinds.get('compose') == 5 and kinds.get('fractal') == 4, (status, kinds)
            assert not errors, errors
            browser.close()
            print('PASS wasm runtime: 6 applications, 5 compositions, 4 Fractal evidence rows without a bridge')
    finally:
        server.shutdown()


if __name__ == '__main__':
    main()
