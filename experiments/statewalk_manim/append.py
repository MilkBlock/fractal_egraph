#!/usr/bin/env python3
"""Append a separately rendered lesson, preserving every original video frame."""
import argparse
from copy import deepcopy
from fractions import Fraction
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys

HERE=Path(__file__).resolve().parent


def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()


def merge_edits(base, extension, frames):
    for key in ['renderer','width','height','fps']:
        if base[key]!=extension[key]:raise ValueError(f'Incompatible {key}')
    offset=frames/base['fps']
    if abs(offset-base['duration'])>1e-5:raise ValueError('Base frame count disagrees with its edit')
    keys=[x['key'] for x in base['captions']+extension['captions']]
    if len(keys)!=len(set(keys)):raise ValueError('Repeated narration beat IDs')
    result=deepcopy(base)
    for kind in ['chapters','captions']:
        for entry in extension[kind]:
            entry=deepcopy(entry);entry['start']+=offset;entry['end']+=offset
            result[kind].append(entry)
    result['duration']=offset+extension['duration']
    return result


def probe(path):
    p=json.loads(subprocess.check_output(['ffprobe','-v','error','-show_streams','-of','json',str(path)],text=True))
    return next(s for s in p['streams'] if s['codec_type']=='video')


def frame_hashes(video, dest, frames):
    subprocess.run(['ffmpeg','-v','error','-y','-i',str(video),'-map','0:v:0','-frames:v',str(frames),'-f','framemd5',str(dest)],check=True)
    return [line.split(',')[-1].strip() for line in dest.read_text().splitlines() if line and not line.startswith('#')]


def main():
    p=argparse.ArgumentParser();p.add_argument('--base',type=Path,required=True);p.add_argument('--extension',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    p.add_argument('--part',choices=['closure','known'],default='closure');a=p.parse_args()
    base,ext,out=[x.resolve() for x in [a.base,a.extension,a.output]]
    if out in [base,ext]:raise ValueError('Use a new output directory')
    old=json.loads((base/'manifest.json').read_text());base_video=base/'statewalk-intuition.mp4'
    if sha(base_video)!=old['sha256']:raise ValueError('Base video changed')
    module,scene=('closure_appendix','ClosureAppendix') if a.part=='closure' else ('known_appendix','KnownAppendix')
    extra_video=ext/'media/videos'/module/'1080p30'/f'{scene}.mp4'
    b,e=probe(base_video),probe(extra_video)
    for key in ['width','height','r_frame_rate','codec_name','pix_fmt','time_base']:
        if b[key]!=e[key]:raise ValueError(f'Incompatible stream {key}')
    assert (b['width'],b['height'],Fraction(b['r_frame_rate']))==(1920,1080,30)
    count=int(b['nb_frames']);extra_count=int(e['nb_frames']);duration=(count+extra_count)/30
    edit=merge_edits(json.loads((base/'edit.json').read_text()),json.loads((ext/'edit.json').read_text()),count)
    assert abs(edit['duration']-duration)<1e-5
    edit['duration']=duration
    out.mkdir(parents=True,exist_ok=True)
    audio=json.loads((base/'audio.json').read_text());extra_audio=json.loads((ext/'audio.json').read_text())
    if audio['speech_settings']!=extra_audio['speech_settings']:raise ValueError('Narration settings differ')
    audio['clips'].update(extra_audio['clips']);audio['beats']+=extra_audio['beats']
    (out/'audio.json').write_text(json.dumps(audio,ensure_ascii=False,indent=2)+'\n')
    (out/'edit.json').write_text(json.dumps(edit,ensure_ascii=False,indent=2)+'\n')
    for name in ['trace-demo.json','ir-demo.json','statewalk-trace.jsonl','vec_add-source.rs','vec_add-extracted.egg','dataflow.json','dataflow_runtime.py','vec_add_python.py','vec_add_plain.py','walkthrough.html']:
        shutil.copy2(base/name,out/name)
    for folder in [base,ext]:
        if (folder/'known-evidence.json').exists():shutil.copy2(folder/'known-evidence.json',out/'known-evidence.json')
    (out/'stills').mkdir(exist_ok=True)
    for folder in [base/'stills',ext/'stills']:
        for f in folder.glob('*.png'):shutil.copy2(f,out/'stills'/f.name)
    def quoted(path):return "file '"+str(path).replace("'","'\\''")+"'\n"
    parts=[]
    for i,video in enumerate([base_video,extra_video]):
        part=out/f'video-part-{i}.mp4'
        subprocess.run(['ffmpeg','-v','error','-y','-i',str(video),'-map','0:v:0','-an','-c:v','copy',str(part)],check=True)
        parts.append(part)
    listing=out/'parts.ffconcat';listing.write_text('ffconcat version 1.0\n'+''.join(map(quoted,parts)))
    joined=out/'joined-video.mp4'
    subprocess.run(['ffmpeg','-v','error','-y','-f','concat','-safe','0','-i',str(listing),'-map','0:v:0','-an','-c:v','copy',str(joined)],check=True)
    assert int(probe(joined)['nb_frames'])==count+extra_count
    raw=out/'media/videos/film/1080p30/StatewalkFilm.mp4';raw.parent.mkdir(parents=True,exist_ok=True)
    base_audio=base/'media/videos/film/1080p30/StatewalkFilm.mp4'
    if not base_audio.exists():base_audio=base_video
    # Every 30-fps frame is exactly 1600 audio samples at 48 kHz.
    filters=(f'[1:a]aresample=48000,apad,atrim=end_sample={count*1600},asetpts=PTS-STARTPTS[a0];'
             f'[2:a]aresample=48000,apad,atrim=end_sample={extra_count*1600},asetpts=PTS-STARTPTS[a1];'
             '[a0][a1]concat=n=2:v=0:a=1[a]')
    subprocess.run(['ffmpeg','-v','error','-y','-i',str(joined),'-i',str(base_audio),'-i',str(extra_video),
                    '-filter_complex',filters,'-map','0:v:0','-map','[a]','-c:v','copy','-c:a','alac',str(raw)],check=True)
    subprocess.run([sys.executable,str(HERE/'finish.py'),'--output',str(out)],check=True)
    final=out/'statewalk-intuition.mp4'
    before=frame_hashes(base_video,out/'base.framemd5',count)
    after=frame_hashes(final,out/'prefix.framemd5',count)
    assert len(before)==len(after)==count and before==after,'Original decoded frames changed'
    manifest=json.loads((out/'manifest.json').read_text())
    manifest['appendix']=dict(part=a.part,start_seconds=count/30,duration_seconds=extra_count/30,
        previous_video_sha256=old['sha256'],new_chapters=len(edit['chapters'])-len(old['chapters']),
        preserved_prefix_frames=count,decoded_prefix_identical=True,
        scope='Appendix uses teaching diagrams and saved vec_add evidence. Known/liveness lessons additionally check all masks against the unchanged native liveness source block. No new inference benchmark. Audio is sample-aligned and normalized for the complete film.')
    manifest['appendices']=old.get('appendices',([old['appendix']] if 'appendix' in old else []))+[manifest['appendix']]
    (out/'manifest.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n')
    page=out/'index.html';html=page.read_text()
    title='Statewalk：数据流、Pure closure 与 If' if a.part=='closure' else 'Statewalk：逐步拆解 known 与 live_known'
    topics='状态元组、纯闭包、成本账本与 If 分支接口' if a.part=='closure' else '加入已知集合、纯闭包逐波传播、未来需求与状态判等'
    html=html.replace('<h1>Statewalk：从搜索到 Python 数据流</h1>',f'<h1>{title}</h1>')
    html=html.replace('<nav>',f'<p>新增内容从 {count//30//60:02}:{count//30%60:02} 开始：{topics}。原版前 {count} 帧逐帧核对一致。</p><nav>')
    page.write_text(html)
    print(json.dumps(manifest['appendix'],ensure_ascii=False))


if __name__=='__main__':main()
