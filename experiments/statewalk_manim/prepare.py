#!/usr/bin/env python3
"""Prepare offline narration and the verified native state graph for Manim CE."""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import sys
from voice import add_options, settings_for, synthesize, description
from pronunciation import spoken_text

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[1]

def main():
    p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True)
    p.add_argument('--trace',type=Path,default=ROOT/'out/statewalk-film-v2-trace-checked')
    p.add_argument('--narration',type=Path,default=HERE/'narration.json')
    add_options(p);a=p.parse_args()
    out=a.output.resolve();(out/'audio').mkdir(parents=True,exist_ok=True)
    data=json.loads(a.narration.read_text());clips={}
    settings=settings_for(a,data);data['narration_label']=description(settings)
    data['speech_settings']=settings
    if settings['backend']=='mambotts':
        data['voice']='MamboTTS/Mambo';data.pop('rate',None);data['speed']=settings['speed']
    sys.path.insert(0,str(ROOT/'experiments/statewalk_video'))
    from trace_data import export
    from real_ir import export as export_ir
    export(a.trace,out,include_all=True);export_ir(ROOT,out)
    for key,caption,subtitle in data['beats']:
        speech=spoken_text(subtitle)
        src=out/'audio'/f'{key}.txt';wav=src.with_suffix('.wav')
        synthesize(speech,wav,settings);src.write_text(speech)
        duration=float(subprocess.check_output(['ffprobe','-v','error','-show_entries','format=duration','-of','default=nw=1:nk=1',str(wav)],text=True))
        clips[key]=dict(caption=caption,subtitle=subtitle,speech=speech,audio=str(wav),duration=duration)
        print(f'[voice] {key}: {duration:.2f}s',flush=True)
    data['clips']=clips
    (out/'audio.json').write_text(json.dumps(data,ensure_ascii=False,indent=2)+'\n')
    shutil.copyfile(a.trace/'trace.jsonl',out/'statewalk-trace.jsonl')
    from dataflow import export as export_dataflow
    export_dataflow(out)
    from walkthrough import export as export_walkthrough
    export_walkthrough(out)
    print(json.dumps(dict(beats=len(clips),speech_seconds=sum(c['duration'] for c in clips.values())),ensure_ascii=False))

if __name__=='__main__':main()
