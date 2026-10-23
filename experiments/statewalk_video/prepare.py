#!/usr/bin/env python3
"""Offline Chinese narration, frame-aligned captions and chapter metadata."""
import argparse
import json
import math
from pathlib import Path
import subprocess
import shutil
from real_ir import export

HERE=Path(__file__).resolve().parent

def timestamp(t):
    ms=round(t*1000);s,ms=divmod(ms,1000);m,s=divmod(s,60);h,m=divmod(m,60)
    return f'{h:02}:{m:02}:{s:02},{ms:03}'

def main():
    p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True)
    p.add_argument('--story',type=Path,default=HERE/'story.json');p.add_argument('--reuse-audio',type=Path);p.add_argument('--trace',type=Path);a=p.parse_args()
    out=a.output.resolve();(out/'audio').mkdir(parents=True,exist_ok=True)
    export(HERE.parents[1],out)
    story=json.loads(a.story.read_text());fps=story['fps'];cursor=0;clips=[];chapters=[];srt=[];reuse={}
    if a.reuse_audio:
        old=json.loads((a.reuse_audio/'timeline.json').read_text())
        assert (old['voice'],old['rate'])==(story['voice'],story['rate'])
        reuse={c.get('speech',c['text']):Path(c['audio']) for c in old['clips']}
    if a.trace:
        from trace_data import export as export_trace
        export_trace(a.trace,out);shutil.copyfile(a.trace/'trace.jsonl',out/'statewalk-trace.jsonl')
    for ch in story['chapters']:
        start=cursor;cursor+=round(.65*fps)
        for text in ch['clips']:
            i=len(clips);source=out/'audio'/f'{i:02}.txt';aiff=source.with_suffix('.aiff');wav=source.with_suffix('.wav')
            changed=not source.exists() or source.read_text()!=text
            source.write_text(text)
            if changed or not wav.exists():
                if text in reuse:shutil.copyfile(reuse[text],wav)
                else:
                    subprocess.run(['say','-v',story['voice'],'-r',str(story['rate']),'-f',str(source),'-o',str(aiff)],check=True)
                    subprocess.run(['ffmpeg','-hide_banner','-loglevel','error','-y','-i',str(aiff),'-ar','48000','-ac','1',str(wav)],check=True)
                    aiff.unlink()
            duration=float(subprocess.check_output(['ffprobe','-v','error','-show_entries','format=duration','-of','default=nw=1:nk=1',str(wav)],text=True))
            caption=text.replace('泰格','Tiger').replace('数值叉','数值 x').replace('叉加一','x 加一').replace('梯','t')
            frames=math.ceil(duration*fps);clip=dict(index=i,text=caption,speech=text,start=cursor,frames=frames,audio=str(wav),chapter=ch['id'])
            clips.append(clip);srt.extend([str(i+1),f'{timestamp(cursor/fps)} --> {timestamp((cursor+frames)/fps)}',caption,''])
            cursor+=frames+round(.3*fps)
        cursor+=round(.8*fps)
        chapters.append(dict(ch,start=start,frames=cursor-start,clips=[c['index'] for c in clips if c['chapter']==ch['id']]))
    metadata=dict(story,chapters=chapters,clips=clips,frames=cursor,duration=cursor/fps,
                  voice_disclosure='本机中文合成语音 · Tingting',scope='教学动画；真实结果来自 f9a6d1c 的 raytrace Pass 3 提取对照')
    (out/'timeline.json').write_text(json.dumps(metadata,ensure_ascii=False,indent=2)+'\n')
    (out/'captions.srt').write_text('\n'.join(srt))
    # Audio is padded to the exact frame timeline, without audio drift at joins.
    segments=[];position=0
    for c in clips:
        if c['start']>position:
            gap=out/'audio'/f'gap-{c["index"]:02}.wav'
            subprocess.run(['ffmpeg','-hide_banner','-loglevel','error','-y','-f','lavfi','-i','anullsrc=r=48000:cl=mono','-t',str((c['start']-position)/fps),str(gap)],check=True)
            segments.append(gap)
        padded=out/'audio'/f'padded-{c["index"]:02}.wav'
        subprocess.run(['ffmpeg','-hide_banner','-loglevel','error','-y','-i',c['audio'],'-af','apad','-t',str(c['frames']/fps),str(padded)],check=True)
        segments.append(padded);position=c['start']+c['frames']
    tail=out/'audio'/'tail.wav'
    subprocess.run(['ffmpeg','-hide_banner','-loglevel','error','-y','-f','lavfi','-i','anullsrc=r=48000:cl=mono','-t',str((cursor-position)/fps),str(tail)],check=True);segments.append(tail)
    concat=out/'audio'/'concat.txt';concat.write_text(''.join("file '"+str(s).replace("'","'\\''")+"'\n" for s in segments))
    subprocess.run(['ffmpeg','-hide_banner','-loglevel','error','-y','-f','concat','-safe','0','-i',str(concat),'-af','loudnorm=I=-18:TP=-2:LRA=7','-ar','48000','-ac','1',str(out/'narration.wav')],check=True)
    chapters_txt=';FFMETADATA1\ntitle=Statewalk：先走对，再算省\ncomment=中文合成配音；教学示例与实测结果已区分\n'
    for c in chapters:
        chapters_txt+=f'[CHAPTER]\nTIMEBASE=1/1000\nSTART={round(c["start"]*1000/fps)}\nEND={round((c["start"]+c["frames"])*1000/fps)}\ntitle={c["section"]} · {c["title"]}\n'
    (out/'chapters.ffmeta').write_text(chapters_txt)
    print(json.dumps(dict(duration=cursor/fps,chapters=len(chapters),clips=len(clips),output=str(out)),ensure_ascii=False))

if __name__=='__main__':main()
