#!/usr/bin/env python3
"""Run actual Manim CE with local TeX paths and an isolated media directory."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

HERE=Path(__file__).resolve().parent

def main():
    p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True);p.add_argument('--quality',choices=['preview','final'],default='final');a=p.parse_args()
    out=a.output.resolve();env=dict(os.environ,STATEWALK_MANIM_DIR=str(out))
    # Homebrew dvisvgm and TeX Live can resolve different kpathsea roots.
    # Scope the correction to this render, without changing system config.
    env['TEXMFROOT']=subprocess.check_output(['kpsewhich','--var-value=TEXMFROOT'],text=True).strip()
    env['TEXMFCNF']=str(Path(subprocess.check_output(['kpsewhich','texmf.cnf'],text=True).strip()).parent)
    preview=a.quality=='preview'
    command=[sys.executable,'-m','manim','render','-ql' if preview else '-qh','--fps','15' if preview else '30','--renderer','cairo','--media_dir',str(out/'media'),'--verbosity','WARNING',str(HERE/'film.py'),'StatewalkFilm']
    (out/f'render-{a.quality}.json').write_text(json.dumps(dict(command=command,tex={k:env[k] for k in ('TEXMFROOT','TEXMFCNF')}),indent=2)+'\n')
    subprocess.run(command,env=env,check=True)

if __name__=='__main__':main()
