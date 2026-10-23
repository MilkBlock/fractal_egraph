#!/usr/bin/env python3
"""Prepare offline narration and the verified native state graph for Manim CE."""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import sys

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[1]

def main():
    p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True)
    p.add_argument('--trace',type=Path,default=ROOT/'out/statewalk-film-v2-trace-checked');a=p.parse_args()
    out=a.output.resolve();(out/'audio').mkdir(parents=True,exist_ok=True)
    data=json.loads((HERE/'narration.json').read_text());clips={}
    sys.path.insert(0,str(ROOT/'experiments/statewalk_video'))
    from trace_data import export
    from real_ir import export as export_ir
    export(a.trace,out,include_all=True);export_ir(ROOT,out)
    for key,caption,speech in data['beats']:
        src=out/'audio'/f'{key}.txt';wav=src.with_suffix('.wav');aiff=src.with_suffix('.aiff')
        if not wav.exists() or not src.exists() or src.read_text()!=speech:
            src.write_text(speech)
            subprocess.run(['say','-v',data['voice'],'-r',str(data['rate']),'-f',str(src),'-o',str(aiff)],check=True)
            subprocess.run(['ffmpeg','-hide_banner','-loglevel','error','-y','-i',str(aiff),'-ar','48000','-ac','1',str(wav)],check=True);aiff.unlink()
        duration=float(subprocess.check_output(['ffprobe','-v','error','-show_entries','format=duration','-of','default=nw=1:nk=1',str(wav)],text=True))
        clips[key]=dict(caption=caption,speech=speech,audio=str(wav),duration=duration)
    data['clips']=clips
    (out/'audio.json').write_text(json.dumps(data,ensure_ascii=False,indent=2)+'\n')
    shutil.copyfile(a.trace/'trace.jsonl',out/'statewalk-trace.jsonl')
    print(json.dumps(dict(beats=len(clips),speech_seconds=sum(c['duration'] for c in clips.values())),ensure_ascii=False))

if __name__=='__main__':main()
