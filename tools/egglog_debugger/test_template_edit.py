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

REWRITE_SOURCE = '''(datatype Expr (Num i64) (Add Expr Expr))
(rewrite (Add (Num a) (Num b)) (Num (+ a b)))
'''

# Two primitive fields on one node must keep two distinct accessors.
COORD_SOURCE = '''; @egg-viz-json {"schema":"egg-viz/v1","kind":"dsl_type","id":"Expr","variants":{"Num":{"fields":["value"],"typst":"{value}","precedence":100},"Coord":{"fields":["x","y"],"typst":"upright(\\"Coord\\")({x}, {y})","precedence":90}}}
(datatype Expr (Num i64) (Coord i64 i64))
(rewrite (Coord p q) (Num (+ p q)))
'''

# The exact wrong annotation shape reported after typing a template into the name field.
WRAPPED_SOURCE = '''; @egg-viz-json {"schema":"egg-viz/v1","kind":"dsl_type","id":"Expr","variants":{"Add":{"fields":["left","right"],"typst":"upright(\\"{left} + {right}\\")({left}, {right})","precedence":90}}}
(datatype Expr (Num i64) (Add Expr Expr))
(rewrite (Add (Num a) (Num b)) (Num (+ a b)))
'''

# A relation action is a conclusion; it used to render as "no conclusion".
LEQ_SOURCE = '''(datatype Expr (Num i64) (Var String) (Add Expr Expr))
(relation leq (Expr Expr))
(rule (
    (= e1 (Num n1))
    (= e2 (Num n2))
    (<= n1 n2)
) (
    (leq e1 e2)
))
'''

# Names with digits inside (`e1a`) are one Typst identifier, not `e_1 a`.
DIGIT_VAR_SOURCE = '''(datatype Expr (Num i64) (Add Expr Expr))
(relation leq (Expr Expr))
(rule (
    (= e1 (Add e1a e1b))
    (= e2 (Add e2a e2b))
    (leq e1a e2a)
    (leq e1b e2b)
) (
    (leq e1 e2)
))
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
            before = page.evaluate('window.egglogNative.rendered')
            page.evaluate('(a)=>{const e=document.querySelector(".CodeMirror").CodeMirror;e.setValue(a.source);e.setCursor({line:a.line,ch:0});}', {'source': source, 'line': line})
            # Wait for a completed render of this exact source, so an opened editor
            # and the toolbar cannot belong to the previous source.
            page.wait_for_function('(a)=>window.egglogNative.rendered>a.before && window.egglogNative.selected && window.egglogNative.selected.preview_source===a.source',
                                   arg={'before': before, 'source': source}, timeout=30000)
            page.wait_for_timeout(150)

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
        # The panel must explain that bare multi-letter names are not Typst math.
        help_text = page.locator('#native-symbol-help').inner_text()
        assert 'upright("Mul")' in help_text and '{{' in help_text, help_text
        assert page.locator('.native-symbol-row[data-template*=\'upright("Mul")\']').count() >= 1
        page.locator('.native-symbol-row', has_text='平方根').first.click()
        assert 'sqrt(' in page.input_value('#native-template-input')
        page.click('#native-template-help')
        assert page.locator('#native-symbol-help').is_hidden()
        page.click('#native-name-cancel')

        # --- invalid edits are rejected and never touch the source ------------
        open_target('constructor:Add')
        page.fill('#native-template-input', 'frac({left}, {right}')
        save_editor()
        page.wait_for_function('()=>document.querySelector("#native-edit-error").textContent.includes("Typst")')
        assert read() == saved
        page.fill('#native-template-input', 'frac({nope}, {right})')
        save_editor()
        page.wait_for_function('()=>document.querySelector("#native-edit-error").textContent.includes("未声明的模板字段")')
        assert read() == saved
        page.fill('#native-name-input', 'Var')
        save_editor()
        page.wait_for_function('()=>document.querySelector("#native-edit-error").textContent.includes("重名")')
        assert read() == saved
        # A bare multi-letter name is auto-corrected into the template box, but the
        # source stays untouched until the user confirms with a second submit.
        page.fill('#native-template-input', 'Mul {{ {left} dot {right} }}')
        save_editor()
        page.wait_for_function('()=>document.querySelector("#native-template-input").value.includes(\'upright("Mul")\')')
        assert read() == saved
        assert 'Mul → upright("Mul")' in page.locator('#native-template-note').inner_text()
        assert page.locator('#native-name-save').inner_text() == '确认修正并保存'
        save_editor()
        page.wait_for_function('()=>document.querySelector(".CodeMirror").CodeMirror.getValue().includes("upright")')
        ready('Mul')
        saved = read()
        assert 'upright(\\"Mul\\")' in saved, saved
        # The confirmed save closes the editor; reopen it for the remaining checks.
        open_target_expect('constructor:Add', '#native-template-input', 'upright("Mul") {{ {left} dot {right} }}')
        page.fill('#native-name-input', 'Plus')
        save_editor()
        ready('Plus')
        assert 'Plus' in read()

        # --- a template typed into the name field stays a template ------------
        set_source(SOURCE, 1)
        ready('Add')
        page.locator('.native-edit-hit[data-target="constructor:Add"]').first.click()
        page.fill('#native-name-input', '{left} + {right}')
        assert page.input_value('#native-template-input') == '{left} + {right}'
        save_editor()
        ready('+')
        annotation = read()
        assert '"typst":"{left} + {right}"' in annotation
        assert 'upright' not in annotation

        # --- rewrite pattern variables are editable variables, not just names ---
        set_source(REWRITE_SOURCE, 1)
        ready('num_node2')
        assert page.locator('.native-edit-hit[data-target="binding:a"]').count() >= 1
        page.locator('.native-edit-hit[data-target="binding:a"]').first.click()
        assert page.input_value('#native-name-input') == 'a'
        page.fill('#native-name-input', 'lhs')
        save_editor()
        page.wait_for_function('()=>document.querySelector(".CodeMirror").CodeMirror.getValue().includes(\'"a":"lhs"\')')
        ready('lhs')
        rendered = page.locator('#native-source').text_content()
        assert 'lhs' in rendered and 'num_node2' not in rendered, rendered
        assert 'num_node3' in rendered
        # A renamed field keeps its node-qualified accessor, named after the variable.
        assert 'lhs.a' in rendered, rendered
        # The other variable is still editable after the first rename.
        page.locator('.native-edit-hit[data-target="binding:b"]').first.click()
        page.fill('#native-name-input', 'rhs')
        save_editor()
        page.wait_for_function('()=>document.querySelector(".CodeMirror").CodeMirror.getValue().includes(\'"b":"rhs"\')')
        ready('rhs')
        rendered = page.locator('#native-source').text_content()
        assert 'lhs.a' in rendered and 'rhs.b' in rendered, rendered
        assert 'arg_i64_00' not in rendered, rendered

        # --- two fields of one node keep two distinct accessors and targets ----
        set_source(COORD_SOURCE, 2)
        ready('coord_node1')
        assert page.locator('.native-edit-hit[data-target="binding:p"]').count() >= 1
        assert page.locator('.native-edit-hit[data-target="binding:q"]').count() == 1
        # Clicking the second field's accessor opens that field, not its sibling.
        page.locator('.native-edit-hit[data-target="binding:q"]').first.click()
        assert page.input_value('#native-name-input') == 'q'
        page.click('#native-name-cancel')
        page.locator('.native-edit-hit[data-target="binding:p"]').first.click()
        page.fill('#native-name-input', 'm')
        save_editor()
        page.wait_for_function('()=>document.querySelector(".CodeMirror").CodeMirror.getValue().includes(\'"p":"m"\')')
        ready('m.q')
        rendered = page.locator('#native-source').text_content()
        assert 'm.p' in rendered and 'm.q' in rendered, rendered
        assert 'arg_i64_0' not in rendered, rendered

        # --- relation actions are conclusions, and the Rust panel shows the scope --
        set_source(LEQ_SOURCE, 3)
        ready('Leq')
        rendered = page.locator('#native-source').text_content()
        assert 'no conclusion' not in rendered, rendered
        assert 'Leq' in rendered, rendered
        assert page.locator('summary', has_text='Rust 源码').count() == 1
        rust = page.locator('#native-rust').text_content()
        assert 'add_rule' in rust and 'insert_leq' in rust, rust

        # --- digit-containing pattern variables are not emitted bare ----------
        set_source(DIGIT_VAR_SOURCE, 2)
        ready('e1a')
        assert not page.locator('#native-render-error').inner_text(), page.locator('#native-render-error').inner_text()
        rendered = page.locator('#native-source').text_content()
        assert 'upright("e1a")' in rendered and 'upright("e1b")' in rendered, rendered
        assert 'Leq' in rendered and 'no conclusion' not in rendered, rendered

        # --- an annotation already wrapped by the old bug is recoverable -------
        set_source(WRAPPED_SOURCE, 2)
        ready('+')
        open_target('constructor:Add')
        assert page.input_value('#native-template-input') == '{left} + {right}'
        save_editor()
        # Wait for the save continuation to rewrite the editor, not for a render that
        # already matched the old source.
        page.wait_for_function('()=>document.querySelector(".CodeMirror").CodeMirror.getValue().includes(\'"typst":"{left} + {right}"\')')
        repaired = read()
        assert '"typst":"{left} + {right}"' in repaired, repaired
        assert 'upright("{left}' not in repaired, repaired

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
