#!/usr/bin/env python3
"""Exercise real glyph hit testing and round-trip .egg annotation editing."""
import argparse
from pathlib import Path
import tempfile
from playwright.sync_api import sync_playwright

SOURCE = '''(datatype Math (Add Math Math) (Var String))
(rule ((= root (Add x y))) ((union root (Add y x))) :name "swap")
(rule ((= root (Add a b))) ((union root (Add b a))) :name "other")
'''


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--url',default='http://127.0.0.1:8080')
    parser.add_argument('--chromium')
    args=parser.parse_args()
    with sync_playwright() as p, tempfile.TemporaryDirectory() as folder:
        browser=p.chromium.launch(headless=True,**({'executable_path':args.chromium} if args.chromium else {}))
        page=browser.new_page()
        errors=[];page.on('pageerror',lambda error:errors.append(str(error)))
        page.goto(args.url,wait_until='networkidle');page.wait_for_selector('#native-run')
        page.evaluate('(source)=>{const e=document.querySelector(".CodeMirror").CodeMirror;e.setValue(source);e.setCursor({line:1,ch:0});}',SOURCE)
        read_source=lambda:page.evaluate('document.querySelector(".CodeMirror").CodeMirror.getValue()')
        def ready(word):
            page.wait_for_function('word=>document.querySelector("#native-preview").dataset.ready === "true" && document.querySelector("#native-source").textContent.includes(word)',arg=word,timeout=30000)
            assert not page.locator('#native-render-error').inner_text()
        def edit(target,value):
            page.locator(f'.native-edit-hit[data-target="{target}"]').first.click()
            page.fill('#native-name-input',value);page.click('#native-name-save')
            ready(value)
        ready('Add')
        edit('constructor:Add','Sum')
        edited=read_source()
        assert 'upright(\\"Sum\\")' in edited
        assert '\n'.join(line for line in edited.split('\n') if not line.startswith('; @egg-viz-json')) == SOURCE
        assert edited.count('; @egg-viz-json') == 1
        # One undo restores the original source, including the absence of metadata.
        page.evaluate('document.querySelector(".CodeMirror").CodeMirror.undo()')
        assert read_source()==SOURCE
        page.evaluate('document.querySelector(".CodeMirror").CodeMirror.redo()')
        assert read_source()==edited
        page.evaluate('document.querySelector(".CodeMirror").CodeMirror.setCursor({line:2,ch:0})')
        ready('Sum')
        edit('binding:x','left_value')
        assert '"x":"left_value"' in read_source()
        assert read_source().count('; @egg-viz-json')==2
        edit('constructor:Add','Combine')
        assert read_source().count('; @egg-viz-json')==2
        saved=read_source()
        # Cancel (including Escape) is a no-op.
        page.locator('.native-edit-hit[data-target="constructor:Add"]').first.click()
        page.fill('#native-name-input','Discarded');page.press('#native-name-input','Escape')
        assert read_source()==saved
        # A concurrent source edit must not be replaced by an older annotation patch.
        page.locator('.native-edit-hit[data-target="constructor:Add"]').first.click()
        page.fill('#native-name-input','Stale')
        page.evaluate(r'''() => {const e=document.querySelector('.CodeMirror').CodeMirror;e.replaceRange('; concurrent edit\n',{line:0,ch:0});document.querySelector('#native-name-editor').requestSubmit();}''')
        assert read_source()=='; concurrent edit\n'+saved
        assert 'Stale' not in read_source()
        # The constructor display is file-wide, the binding display is rule-local.
        page.evaluate(r'''() => {const e=document.querySelector('.CodeMirror').CodeMirror;const line=e.getValue().split('\n').findIndex(s=>s.includes(':name "other"'));e.setCursor({line,ch:0});}''')
        page.wait_for_function('window.egglogNative.selected?.rule === "other"')
        ready('Combine')
        assert 'left_value' not in page.locator('#native-source').text_content()
        with page.expect_download() as download:page.click('#native-download')
        file=Path(folder)/'edited.egg';download.value.save_as(file)
        assert file.read_text()==read_source()
        # The default demo's annotations are present in the menu's actual payload.
        examples=page.request.get(args.url+'/examples.json').json()
        assert len(examples)==54
        assert '@egg-viz-json' in examples['01-basics'] and 'dsl_type' in examples['math']
        assert not errors, errors
        browser.close()
        print('PASS glyph editing: constructor, binding, source round-trip, undo/redo, cancel, stale-source guard, scope, .egg download')

if __name__=='__main__':main()
