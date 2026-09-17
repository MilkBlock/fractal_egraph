#!/usr/bin/env python3
"""Regression for Typst template editing, validation, help popup and re-editing.

Covers the reported failure where a rule could only be edited once, because a
plugin text fallback (or a template without a literal name) removed every glyph
hit region.
"""
import argparse
import json
from pathlib import Path
from playwright.sync_api import sync_playwright

SOURCE = '''(datatype Math (Add Math Math) (Var String))
(rule ((= root (Add x y))) ((union root (Add y x))) :name "swap")
(rule ((= root (Add a b))) ((union root (Add b a))) :name "other")
'''

CONDITION_SOURCE = '''(datatype Math (Num i64) (Fabs Math))
(rewrite (Fabs (Num a)) (Num (abs a)))
'''

EXAMPLES = Path(__file__).resolve().parents[3] / 'egglog-demo/static/examples.json'
HERBIE = json.loads(EXAMPLES.read_text())['herbie'] if EXAMPLES.is_file() else None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--url', default='http://127.0.0.1:8080')
    parser.add_argument('--chromium')
    args = parser.parse_args()
    with sync_playwright() as p:
        browser = p.chromium.launch(headless=True, **({'executable_path': args.chromium} if args.chromium else {}))
        page = browser.new_page()
        errors = []
        page.on('pageerror', lambda error: errors.append(str(error)))
        page.goto(args.url, wait_until='networkidle')
        page.wait_for_selector('#native-run')

        def set_source(source, line=1):
            page.evaluate('(a)=>{const e=document.querySelector(".CodeMirror").CodeMirror;e.setValue(a.source);e.setCursor({line:a.line,ch:0});}', {'source': source, 'line': line})

        def read():
            return page.evaluate('document.querySelector(".CodeMirror").CodeMirror.getValue()')

        def ready(word=''):
            page.wait_for_function('w=>document.querySelector("#native-preview").dataset.ready==="true" && (!w || document.querySelector("#native-source").textContent.includes(w))', arg=word, timeout=30000)

        def open_target(target):
            page.locator(f'#native-targets button[data-target="{target}"]').first.click()

        def open_target_expect(target, selector, expected, attempts=25):
            # The preview re-render is debounced; retry until the fresh target wins.
            for _ in range(attempts):
                open_target(target)
                if page.input_value(selector) == expected:
                    return
                page.wait_for_timeout(200)
            raise AssertionError(f'{target} {selector}={page.input_value(selector)!r}, expected {expected!r}')

        def save_editor():
            page.click('#native-name-save')

        # --- edit a constructor template and keep editing it afterwards --------
        set_source(SOURCE, 1)
        ready('Add')
        page.locator('.native-edit-hit[data-target="constructor:Add"]').first.click()
        assert page.locator('#native-template-editor').is_visible()
        assert page.input_value('#native-template-input')
        page.fill('#native-template-input', 'frac({left}, {right})')
        save_editor()
        page.wait_for_function('()=>document.querySelector(".CodeMirror").CodeMirror.getValue().includes("frac({left}, {right})")')
        ready('frac')
        # The template has no constructor name left, so the target list is the only
        # reliable way in; it must exist and open the current template.
        open_target_expect('constructor:Add', '#native-template-input', 'frac({left}, {right})')
        page.fill('#native-template-input', '{left} + {right}')
        page.fill('#native-precedence-input', '50')
        save_editor()
        page.wait_for_function('()=>document.querySelector(".CodeMirror").CodeMirror.getValue().includes("{left} + {right}")')
        ready('+')
        saved = read()
        open_target_expect('constructor:Add', '#native-template-input', '{left} + {right}')
        assert page.input_value('#native-precedence-input') == '50'

        # --- help popup lists symbols and inserts a template -------------------
        assert page.locator('#native-symbol-help').is_hidden()
        page.click('#native-template-help')
        assert page.locator('#native-symbol-help').is_visible()
        assert page.locator('.native-symbol-row').count() >= 20
        page.locator('.native-symbol-row', has_text='平方根').first.click()
        assert 'sqrt(' in page.input_value('#native-template-input')
        page.click('#native-template-help')
        assert page.locator('#native-symbol-help').is_hidden()
        page.click('#native-name-cancel')

        # --- invalid edits are rejected and never touch the source ------------
        open_target('constructor:Add')
        page.fill('#native-template-input', 'nosuchfn({left}, {right})')
        save_editor()
        page.wait_for_function('()=>document.querySelector("#native-edit-error").textContent.length>0')
        assert 'Typst' in page.locator('#native-edit-error').inner_text()
        assert read() == saved
        page.fill('#native-template-input', 'frac({nope}, {right})')
        save_editor()
        page.wait_for_function('()=>document.querySelector("#native-edit-error").textContent.includes("未声明的模板字段")')
        assert read() == saved
        page.fill('#native-name-input', 'Var')
        save_editor()
        page.wait_for_function('()=>document.querySelector("#native-edit-error").textContent.includes("重名")')
        assert read() == saved
        # A rejected edit keeps the editor open, so the next attempt still works.
        page.fill('#native-name-input', 'Plus')
        save_editor()
        ready('Plus')
        assert 'Plus' in read()

        # --- a condition can be edited more than once -------------------------
        set_source(CONDITION_SOURCE, 1)
        ready('Fabs')
        page.locator('.native-edit-hit[data-target="condition:when"]').first.click()
        page.fill('#native-condition-input', '(> a 0)')
        page.click('#native-condition-save')
        page.wait_for_function('()=>document.querySelector(".CodeMirror").CodeMirror.getValue().includes("(> a 0)")')
        page.wait_for_timeout(800)
        ready('Fabs')
        # This plugin renders the condition as text, so glyph hit testing disappears;
        # the target list must still allow a second edit (the reported "once only" bug).
        open_target_expect('condition:when', '#native-condition-input', '(> a 0)')
        page.fill('#native-condition-input', '(>= a 1)')
        page.click('#native-condition-save')
        page.wait_for_function('()=>document.querySelector(".CodeMirror").CodeMirror.getValue().includes("(>= a 1)")')
        assert '(> a 0)' not in read()

        # --- clicked rule line still maps to its own rule ---------------------
        if HERBIE is not None:
            set_source(HERBIE, 74)
            ready('Fabs')
            assert 'Floor' not in page.locator('#native-source').text_content()
            page.evaluate('()=>{document.querySelector(".CodeMirror").CodeMirror.setCursor({line:72,ch:0});}')
            page.wait_for_function('()=>document.querySelector("#native-title").textContent.includes("没有 rule/rewrite")')
        assert not errors, errors
        browser.close()
        print('PASS template editing: validation, help popup, repeated edits, condition re-edit, rule mapping')


if __name__ == '__main__':
    main()
