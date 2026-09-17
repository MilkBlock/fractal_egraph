#!/usr/bin/env python3
"""Real Chromium integration: source clicks, streaming, replay, stale views and errors."""
import argparse
import json
from pathlib import Path
import tempfile
from playwright.sync_api import sync_playwright

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--url', default='http://127.0.0.1:8080')
    parser.add_argument('--chromium', help='Optional installed Chromium executable')
    args = parser.parse_args()
    with sync_playwright() as playwright, tempfile.TemporaryDirectory() as folder:
        browser = playwright.chromium.launch(headless=True, **({'executable_path': args.chromium} if args.chromium else {}))
        page = browser.new_page(viewport={'width': 1440, 'height': 1000})
        errors = []
        page.on('pageerror', lambda error: errors.append(str(error)))
        page.goto(args.url, wait_until='networkidle')
        page.wait_for_selector('#native-run')
        source = '; 中文源码\n' + (ROOT / 'experiments/bake/increment-3.egg').read_text()
        set_source = '(s)=>document.querySelector(".CodeMirror").CodeMirror.setValue(s)'
        page.evaluate(set_source, source)
        page.evaluate('document.querySelector(".CodeMirror").CodeMirror.setCursor({line:3,ch:2})')
        def rendered():
            page.wait_for_function('!document.querySelector("#native-preview").hidden && document.querySelector("#native-preview").dataset.ready === "true"')
            assert page.locator('#native-render-error').inner_text() == ''
        page.wait_for_function('window.egglogNative.selected?.rule === "advance"')
        rendered()
        assert 'frac(' in page.locator('#native-source').text_content()
        assert 'eggplant-pattern-vscode' in page.locator('#native-renderer').inner_text()
        page.select_option('#native-format', 'dot')
        rendered()
        assert 'digraph EggplantPattern' in page.locator('#native-source').text_content()
        assert page.locator('#native-preview svg').count() == 1
        page.select_option('#native-format', 'typst')
        page.click('#native-run')
        page.wait_for_function('document.querySelector("#native-status").textContent.startsWith("完成")', timeout=60000)
        rows = page.evaluate('window.egglogNative.rows')
        assert [sum(r['kind'] == kind for r in rows) for kind in ['application', 'compose', 'fractal']] == [6, 5, 4]
        assert len({r['id'] for r in rows}) == len(rows)
        # Every kind of event uses the same plugin pipeline. No legacy formula
        # is compiled by the browser, including staged compositions and fractals.
        for kind in ['application', 'compose', 'fractal']:
            page.select_option('#native-filter', kind)
            page.locator('#native-trace button').last.click()
            rendered()
            assert 'frac(' in page.locator('#native-source').text_content()
            page.select_option('#native-format', 'dot')
            rendered()
            assert 'digraph EggplantPattern' in page.locator('#native-source').text_content()
            assert page.locator('#native-preview image[data-typst-rendering]').count() > 0
            page.select_option('#native-format', 'typst')
            rendered()
        page.select_option('#native-filter', 'fractal')
        page.locator('#native-trace button').last.click()
        rendered()
        original = page.locator('#native-source').text_content()
        assert 'FractalComb' in page.locator('#native-evidence').inner_text()
        # A fractal lane is one repetition statement: the formula keeps the rule
        # and appends the depth/operator/update badge instead of one step per
        # event (up to 168 of them).
        assert 'FractalComb' in original and 'arrow.l' in original
        assert page.locator('#native-step option').count() == 1
        assert '×5' in page.locator('#native-step option').first.text_content()
        page.screenshot(path=str(Path(folder) / 'fractal.png'), full_page=True)
        with page.expect_download() as download:
            page.click('#native-export')
        saved = Path(folder) / 'history.json'
        download.value.save_as(saved)
        assert json.loads(saved.read_text())['version'] == 2
        assert json.loads(saved.read_text())['rows'] == page.evaluate('window.egglogNative.rows')
        # A source edit must not corrupt a historical row or jump to an unrelated line.
        page.evaluate(set_source, '(datatype Math (Const i64))\n')
        page.wait_for_function('document.querySelector("#native-title").textContent.includes("没有 rule")')
        page.locator('#native-trace button').last.click()
        rendered()
        assert page.locator('#native-source').text_content() == original
        # Reload: imported logs carry all formula data, with no runtime rerun.
        page.reload(wait_until='networkidle')
        page.wait_for_selector('#native-run')
        page.set_input_files('#native-import', str(saved))
        page.wait_for_function('window.egglogNative.rows.length === 15')
        page.select_option('#native-filter', 'fractal')
        page.locator('#native-trace button').last.click()
        rendered()
        assert page.locator('#native-source').text_content() == original
        page.click('#native-restore')
        assert page.evaluate('document.querySelector(".CodeMirror").CodeMirror.getValue()') == source
        # Two distinct multiline rules are selected through their interior source lines.
        preview_source = '; 中文\n(datatype Math (Const i64) (Add Math Math))\n(rewrite\n (Add x (Const 0))\n x :name "zero")\n(rewrite\n (Add (Const 0) x)\n x :name "left")\n'
        page.evaluate(set_source, preview_source)
        page.evaluate('document.querySelector(".CodeMirror").CodeMirror.setCursor({line:3,ch:2})')
        page.wait_for_function('window.egglogNative.selected?.rule === "zero"')
        rendered()
        first = page.locator('#native-source').text_content()
        page.evaluate('document.querySelector(".CodeMirror").CodeMirror.setCursor({line:6,ch:2})')
        page.wait_for_function('window.egglogNative.selected?.rule === "left"')
        rendered()
        assert first != page.locator('#native-source').text_content()
        # Empty/unsupported runs must clear old results and surface an honest failure.
        page.evaluate(set_source, '(datatype Other (A i64))\n(run 1)')
        page.click('#native-run')
        page.wait_for_function('document.querySelector("#native-status").textContent.startsWith("运行失败")')
        assert page.evaluate('window.egglogNative.rows.length') == 0
        # Cancel a long, productive native run, then successfully start a fresh one.
        long_source = '(datatype Math (A i64)) (A 0) (rule ((= x (A n))) ((A (+ n 1))) :name "forever") (run 1000000)'
        page.evaluate(set_source, long_source)
        page.click('#native-run')
        page.wait_for_function('window.egglogNative.rows.length > 0')
        page.click('#native-stop')
        page.wait_for_function('document.querySelector("#native-status").textContent.startsWith("已停止")')
        page.evaluate(set_source, source)
        page.click('#native-run')
        page.wait_for_function('document.querySelector("#native-status").textContent.startsWith("完成")', timeout=60000)
        # Upstream Run still works and does not overwrite native history.
        page.evaluate(set_source, '(datatype Math (Const i64)) (Const 7) (extract (Const 7))')
        page.get_by_role('button', name='Run', exact=True).click()
        page.wait_for_function('window.lastMessage?.data.text.includes("Const 7")', timeout=30000)
        assert page.evaluate('window.egglogNative.rows.length') == 15
        assert not errors, errors
        browser.close()
        print('PASS: line previews, both renderers, native deltas, snapshot replay, errors, cancellation, and upstream WASM')

if __name__ == '__main__':
    main()
