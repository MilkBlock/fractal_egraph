#!/usr/bin/env python3
"""Package Manim frames, normalized narration, chapters and a local player."""
import argparse
from fractions import Fraction
import hashlib
import html
import json
from pathlib import Path
import subprocess


def timestamp(seconds, separator=','):
    ms = round(seconds * 1000)
    return f'{ms//3600000:02}:{ms//60000%60:02}:{ms//1000%60:02}{separator}{ms%1000:03}'


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    out = args.output.resolve()
    edit = json.loads((out/'edit.json').read_text())
    assert (edit['width'], edit['height'], edit['fps']) == (1920, 1080, 30)
    duration = round(edit['duration'], 3)
    cues = []
    for i, cue in enumerate(edit['captions'], 1):
        assert 0 <= cue['start'] < cue['end'] <= duration
        if i > 1:
            assert edit['captions'][i-2]['end'] <= cue['start']
        # Full narration accompanies the shorter burned-in visual explanation.
        line = cue['text']
        parts = [line[n:n+26] for n in range(0, len(line), 26)]
        cues.append(f"{i}\n{timestamp(cue['start'])} --> {timestamp(cue['end'])}\n" + '\n'.join(parts))
    (out/'captions.srt').write_text('\n\n'.join(cues)+'\n', encoding='utf-8')
    (out/'captions.vtt').write_text('WEBVTT\n\n'+'\n\n'.join(
        f"{timestamp(c['start'], '.')} --> {timestamp(c['end'], '.')}\n{c['text']}"
        for c in edit['captions'])+'\n', encoding='utf-8')
    metadata = [';FFMETADATA1', 'title=Statewalk：带着钥匙寻找路径',
                'comment=Original animation rendered with Manim Community 0.19.0. Chinese narration synthesized with macOS Tingting.']
    for chapter in edit['chapters']:
        metadata += ['[CHAPTER]', 'TIMEBASE=1/1000', f"START={round(chapter['start']*1000)}",
                     f"END={round(chapter['end']*1000)}", 'title='+chapter['title']]
    (out/'chapters.ffmetadata').write_text('\n'.join(metadata)+'\n')
    raw = out/'media/videos/film/1080p30/StatewalkFilm.mp4'
    final = out/'statewalk-intuition.mp4'
    subprocess.run(['ffmpeg', '-hide_banner', '-loglevel', 'error', '-y',
                    '-i', str(raw), '-i', str(out/'captions.srt'),
                    '-f', 'ffmetadata', '-i', str(out/'chapters.ffmetadata'),
                    '-map', '0:v:0', '-map', '0:a:0', '-map', '1:0',
                    '-map_metadata', '2', '-map_chapters', '2', '-c:v', 'copy',
                    '-af', f'loudnorm=I=-18:TP=-2:LRA=8,apad,atrim=duration={duration}',
                    '-c:a', 'aac', '-ar', '48000', '-b:a', '128k',
                    '-c:s', 'mov_text', '-metadata:s:s:0', 'language=zho',
                    '-metadata:s:a:0', 'language=zho', '-disposition:s:0', '0',
                    '-movflags', '+faststart', str(final)], check=True)
    probe = json.loads(subprocess.check_output(['ffprobe', '-v', 'error', '-show_streams',
                                              '-show_format', '-show_chapters', '-of', 'json', str(final)], text=True))
    video = next(s for s in probe['streams'] if s['codec_type']=='video')
    audio = next(s for s in probe['streams'] if s['codec_type']=='audio')
    assert (video['width'],video['height'],Fraction(video['r_frame_rate'])) == (1920,1080,30)
    assert int(video['nb_frames']) == round(duration*30)
    assert abs(float(video['duration'])-float(audio['duration'])) < .1
    assert len(probe['chapters']) == len(edit['chapters']) == 9
    # Decode the complete final film: metadata checks alone miss damaged frames.
    subprocess.run(['ffmpeg','-v','error','-xerror','-i',str(final),'-f','null','-'],check=True)
    subprocess.run(['ffmpeg','-v','error','-y','-i',str(out/'stills/01-opening.png'),
                    '-frames:v','1',str(out/'poster.jpg')],check=True)
    trace = json.loads((out/'trace-demo.json').read_text())
    manifest = dict(schema='statewalk-manim/v1', renderer=edit['renderer'],
                    duration_seconds=duration, width=1920, height=1080, fps=30,
                    frames=int(video['nb_frames']), chapters=edit['chapters'],
                    bytes=final.stat().st_size, sha256=hashlib.sha256(final.read_bytes()).hexdigest(),
                    full_decode_passed=True, narration='Synthesized Chinese: macOS Tingting',
                    native_trace=trace['summary'],
                    scope='Actual native Statewalk decisions; illustrative cost tokens. Reuses saved single-run CPU extraction measurements; no new timing or GPU claim.')
    (out/'manifest.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n')
    buttons='\n'.join(f'<button data-time="{c["start"]:.3f}"><span>{int(c["start"])//60:02}:{int(c["start"])%60:02}</span>{html.escape(c["title"])}</button>' for c in edit['chapters'])
    page='''<!doctype html><html lang="zh-CN"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Statewalk · 带着钥匙寻找路径</title><style>
:root{color-scheme:dark;font-family:"PingFang SC",system-ui,sans-serif;background:#101014;color:#eee}*{box-sizing:border-box}body{margin:0}main{max-width:1320px;margin:auto;padding:36px 30px 60px}header small{color:#58c4dd;letter-spacing:.15em}h1{font-size:clamp(25px,4vw,44px);font-weight:500;margin:14px 0}p{color:#b8b8c5;line-height:1.8}video{display:block;width:100%;background:#101014;aspect-ratio:16/9;border:1px solid #30303a;border-radius:10px}nav{display:grid;grid-template-columns:repeat(3,1fr);gap:8px;margin:18px 0 28px}button{background:#1b1b23;color:#d8d8e5;border:1px solid #333342;border-radius:8px;text-align:left;padding:14px;cursor:pointer;font:inherit;font-size:14px}button:hover,button.active{border-color:#58c4dd;background:#18262d}button span{color:#58c4dd;display:inline-block;margin-right:12px;font-variant-numeric:tabular-nums}a{color:#83c167;text-underline-offset:4px}footer{border-top:1px solid #30303a;padding-top:16px;font-size:14px}.links{display:flex;gap:22px;flex-wrap:wrap}@media(max-width:800px){main{padding:24px 14px}nav{grid-template-columns:1fr 1fr}}@media(max-width:500px){nav{grid-template-columns:1fr}}
</style><main><header><small>MANIM COMMUNITY · 原创数学动画</small><h1>Statewalk：带着钥匙寻找路径</h1><p>从真实 eggcc 向量加法出发，看清“可用值”、合法路径，以及共享成本为什么只付一次。6分32秒 · 1080p / 30fps · 中文合成旁白</p></header>
<video id="film" controls playsinline preload="metadata" poster="poster.jpg"><source src="statewalk-intuition.mp4" type="video/mp4"><track kind="subtitles" src="captions.vtt" srclang="zh" label="完整中文旁白"></video>
<nav>__BUTTONS__</nav><div class="links"><a href="statewalk-intuition.mp4" download>下载视频</a><a href="captions.srt" download>完整中文字幕</a><a href="vec_add-source.rs">真实 Rust 源码</a><a href="vec_add-extracted.egg">对应 eggcc IR</a><a href="statewalk-trace.jsonl">原生搜索记录</a><a href="manifest.json">验证与来源</a></div>
<footer><p>808 个状态、1,864 次候选检查来自同一段真实 Statewalk 记录。筹码成本 3、1、2 是教学数值。407→29 ms 与 2.272→1.735 s 来自已保存的单次 Tiger 对照，不是新 benchmark，也不是 GPU 加速。</p><p>中文旁白由 macOS Tingting 合成；画面由 Manim Community 0.19.0 制作。采用几何对象连续变形的讲解方式，无借用镜头或他人配音。完整旁白字幕可用播放器字幕菜单开启，浏览器对本地字幕的支持不同，独立 SRT 始终可下载。</p></footer></main>
<script>const film=document.querySelector('#film'),buttons=[...document.querySelectorAll('nav button')];buttons.forEach(b=>b.onclick=()=>{film.currentTime=Number(b.dataset.time);film.play().catch(()=>{});film.scrollIntoView({behavior:'smooth',block:'center'});});film.addEventListener('timeupdate',()=>buttons.forEach((b,i)=>b.classList.toggle('active',film.currentTime>=Number(b.dataset.time)&&(i===buttons.length-1||film.currentTime<Number(buttons[i+1].dataset.time)))));</script></html>'''
    (out/'index.html').write_text(page.replace('__BUTTONS__',buttons), encoding='utf-8')
    print(json.dumps({k:manifest[k] for k in ('renderer','duration_seconds','frames','bytes','sha256','full_decode_passed')},ensure_ascii=False))


if __name__=='__main__':
    main()
