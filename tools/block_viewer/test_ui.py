from pathlib import Path
from playwright.sync_api import sync_playwright
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'results/block_viewer'
errors=[]
with sync_playwright() as p:
    browser=p.chromium.launch(channel='chrome',headless=True)
    page=browser.new_page(viewport={'width':1280,'height':1050},device_scale_factor=1,color_scheme='light')
    page.on('pageerror',lambda e:errors.append(str(e)))
    page.goto((OUT/'index.html').as_uri());page.wait_for_load_state('networkidle')
    assert page.locator('.block').count()==1
    assert page.locator('[data-highlight="true"]').count()==3
    page.select_option('#prefix','0')
    assert page.locator('[data-highlight="true"]').count()==2
    page.select_option('#phase','0');assert page.locator('.block').count()==0
    assert page.locator('#rejections').count()==1
    page.select_option('#phase','2')
    (OUT/'screenshots').mkdir(exist_ok=True)
    page.screenshot(path=str(OUT/'screenshots/chain.png'),full_page=True)
    page.select_option('#scenario','1');page.select_option('#phase','4')
    assert page.locator('.block').count()==3
    page.wait_for_selector('svg.links')
    assert page.locator('svg.links > path').count()==2
    page.select_option('#block','2');assert page.locator('#prefix').is_disabled()
    assert 'Block 0' in page.locator('#detail').inner_text() and 'Block 1' in page.locator('#detail').inner_text()
    page.screenshot(path=str(OUT/'screenshots/join.png'),full_page=True)
    page.select_option('#scenario','2')
    roots=page.locator('.roots strong').all_text_contents();assert len(set(roots))==2
    page.select_option('#phase','1')
    assert len(set(page.locator('.roots strong').all_text_contents()))==1
    assert page.locator('.block.dirty').count()==2
    values=[]
    for b in ['0','1']:
        page.select_option('#block',b);values.append(page.locator('#detail .selection tbody tr td:nth-child(3)').inner_text())
    assert values[0]!=values[1]
    page.screenshot(path=str(OUT/'screenshots/partial_union.png'),full_page=True)
    page.select_option('#phase','2');values=[]
    for b in ['0','1']:
        page.select_option('#block',b);values.append(page.locator('#detail .selection tbody tr td:nth-child(3)').inner_text())
    assert values[0]==values[1]
    assert page.locator('.block').count()==2
    page.set_viewport_size({'width':390,'height':844})
    assert page.locator('body').evaluate('(e)=>e.scrollWidth')<=390
    page.screenshot(path=str(OUT/'screenshots/mobile.png'),full_page=True)
    page.emulate_media(color_scheme='dark');page.set_viewport_size({'width':1280,'height':1050})
    page.screenshot(path=str(OUT/'screenshots/dark.png'),full_page=True)
    page.locator('#load').set_input_files(str(OUT/'data.json'));page.wait_for_timeout(100)
    assert page.locator('#error').inner_text()==''
    page.goto((OUT/'cyk.html').as_uri())
    page.select_option('#phase','0')
    assert page.locator('.block').count()>0
    assert 'action lane did not survive' in page.locator('#rejections').inner_text()
    page.select_option('#phase','1')
    assert page.locator('.block:not(.dirty)').count()==0
    page.goto((OUT/'union.html').as_uri())
    page.select_option('#phase','2')
    assert page.locator('.block').count()==2
    assert '发生合并' in page.locator('#rejections').inner_text()
    assert 'Inserted' in page.locator('#rejections').inner_text()
    assert not errors,errors
    browser.close()
print('UI checks passed: prefix highlighting, phases, block selection, join edges, two union stages, upload, mobile and dark mode; no page errors.')
