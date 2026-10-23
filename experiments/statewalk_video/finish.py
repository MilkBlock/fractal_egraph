#!/usr/bin/env python3
"""Mux narration/chapters/subtitles and make a local chapter player."""
import argparse
import hashlib
import html
import json
from pathlib import Path
import subprocess

def main():
    p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True);a=p.parse_args();out=a.output.resolve()
    data=json.loads((out/'timeline.json').read_text());video=out/'statewalk-explained.mp4'
    subprocess.run(['ffmpeg','-hide_banner','-loglevel','error','-y','-i',str(out/'silent.mp4'),'-i',str(out/'narration.wav'),'-i',str(out/'captions.srt'),'-i',str(out/'chapters.ffmeta'),'-map','0:v','-map','1:a','-map','2:s','-map_metadata','3','-map_chapters','3','-c:v','copy','-c:a','aac','-b:a','160k','-c:s','mov_text','-metadata:s:a:0','language=zho','-metadata:s:s:0','language=zho','-disposition:s:0','0','-movflags','+faststart','-t',str(data['duration']),str(video)],check=True)
    info=json.loads(subprocess.check_output(['ffprobe','-v','error','-show_format','-show_streams','-show_chapters','-of','json',str(video)],text=True))
    v=next(s for s in info['streams'] if s['codec_type']=='video');audio=next(s for s in info['streams'] if s['codec_type']=='audio')
    assert v['width']==1920 and v['height']==1080 and v['nb_frames']==str(data['frames'])
    assert abs(float(info['format']['duration'])-data['duration'])<.1
    assert abs(float(audio['duration'])-data['duration'])<.1
    assert len(info['chapters'])==len(data['chapters'])
    items=[]
    for c in data['chapters']:
        sec=c['start']/data['fps'];label=f'{int(sec)//60:02}:{int(sec)%60:02}'
        items.append(f'<button data-time="{sec}"><time>{label}</time><span>{html.escape(c["title"])}</span></button>')
    page='''<!doctype html><html lang="zh-CN"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Statewalk · 先走对，再算省</title>
<style>*{box-sizing:border-box}body{margin:0;background:#08121f;color:#eef5fc;font:16px -apple-system,BlinkMacSystemFont,"PingFang SC",sans-serif}main{max-width:1480px;margin:auto;padding:44px 32px}header{display:flex;justify-content:space-between;align-items:center}small{color:#55e1e6;letter-spacing:.15em}h1{font-size:40px;margin:14px 0}p{color:#99acc0;line-height:1.6}video{display:block;width:100%;background:#050b13;border-radius:18px;border:1px solid #294059}section{display:grid;grid-template-columns:minmax(0,1fr) 310px;gap:24px;margin-top:30px}nav{display:grid;gap:7px;align-content:start}button{display:flex;gap:14px;align-items:center;border:1px solid #26394e;color:#eef5fc;background:#101f31;padding:13px 14px;border-radius:10px;text-align:left;cursor:pointer;font:inherit;font-size:14px}button:hover,button.active{border-color:#55e1e6;background:#14313c}time{color:#55e1e6;font:13px monospace}a{color:#b7ef99;text-decoration:none}.links{display:flex;gap:24px;margin:20px 0}.note{font-size:13px;color:#99acc0;border-top:1px solid #26394e;padding-top:18px} @media(max-width:1000px){section{grid-template-columns:1fr}nav{grid-template-columns:repeat(2,1fr)}h1{font-size:28px}} </style>
<main><header><div><small>FRACTAL EGRAPH / RESEARCH FILM</small><h1>Statewalk：先走对，再算省</h1><p>合法的副作用路径，正确的共享成本，以及一项保持答案的实现优化。</p></div></header>
<section><div><video id="film" controls playsinline preload="metadata" poster="poster.jpg" src="statewalk-explained.mp4"></video><div class="links"><a href="statewalk-explained.mp4" download>下载 1080p 视频</a><a href="captions.srt" download>下载中文字幕</a><a href="vec_add-extracted.egg" download>真实 vec_add IR</a></div><p class="note">DURATION_LABEL · CHAPTER_LABEL章节 · 本机中文合成配音（Tingting）<br>包含真实 raytrace / vec_add 的源码、原始IR和状态路径。教学示例与真实测量已区分。主要收益来自CPU共享成本表示，尚未接入matching marker或在线extraction。</p></div><nav>'''+''.join(items)+'''</nav></section></main>
<script>const v=document.querySelector('video'),buttons=[...document.querySelectorAll('button')];buttons.forEach(b=>b.onclick=()=>{v.currentTime=+b.dataset.time;v.play()});v.ontimeupdate=()=>{let i=buttons.findLastIndex(b=>+b.dataset.time<=v.currentTime);buttons.forEach((b,j)=>b.classList.toggle('active',i===j))};</script></html>'''
    page=page.replace('DURATION_LABEL',f'{int(data["duration"])//60}分{int(data["duration"])%60:02}秒').replace('CHAPTER_LABEL',str(len(data['chapters'])))
    trace_file=out/'trace-demo.json'
    if trace_file.exists():
        trace=json.loads(trace_file.read_text());s=trace['summary']
        page=page.replace('真实 vec_add IR</a>', '真实 vec_add IR</a><a href="statewalk-trace.jsonl" download>原生搜索记录</a>')
        evidence=f'<p class="note">第二版 · 真实 Statewalk 记录<br>{trace["classes"]} 个e-class · {trace["enodes"]} 个e-node · {s["done"]["states"]} 个搜索状态<br>{s["counts"]["candidate"]} 次候选检查 · {s["missing_dependencies"]} 次依赖未齐 · 纯闭包缓存命中 {s["closure_cache_hits"]} 次<br>插桩前后提取文件逐字节相同；本页计数不代表性能测量。</p>'
        page=page.replace('</video>', '</video>'+evidence)
    (out/'index.html').write_text(page)
    manifest=dict(video=str(video),duration=data['duration'],width=1920,height=1080,fps=data['fps'],frames=data['frames'],chapters=len(data['chapters']),voice=data['voice_disclosure'],bytes=video.stat().st_size,sha256=hashlib.sha256(video.read_bytes()).hexdigest(),verified='ffprobe: video/audio length, frame count, dimensions and chapter count',sources=['check_statewalk@ae80f82a/dag_in_context/src/tiger/statewalkdp.cpp','check_statewalk@ae80f82a/dag_in_context/src/tiger/greedy.cpp','egg_layout@f9a6d1c/experiments/tiger_extraction_markers/results.json'])
    manifest['real_ir']=json.loads((out/'ir-demo.json').read_text())
    if trace_file.exists():manifest['native_trace']=json.loads(trace_file.read_text())['summary']
    (out/'manifest.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n');print(json.dumps(manifest,ensure_ascii=False,indent=2))

if __name__=='__main__':main()
