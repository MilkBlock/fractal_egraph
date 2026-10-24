"""Narration adapters: macOS say or MamboTTS's existing GPT-SoVITS API.

Uses the legacy POST / contract documented by Tsukimisaka/MamboTTS. A running
engine must already have the intended voice weights and default reference loaded.
No automatic model switching, installer execution or silent voice fallback.
"""
import argparse
import hashlib
import http.client
import json
from pathlib import Path
import subprocess
import tempfile
import urllib.error
import urllib.request
import wave
from pronunciation import spoken_text


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def synthesize(text, destination, settings):
    destination=Path(destination);destination.parent.mkdir(parents=True,exist_ok=True)
    signature=dict(text=text,**settings)
    cache=destination.with_suffix('.voice.json')
    if destination.exists() and cache.exists():
        cached=json.loads(cache.read_text())
        if cached.get('request')==signature and cached.get('sha256')==digest(destination):
            return False
    with tempfile.TemporaryDirectory(prefix='speech-',dir=destination.parent) as tmp:
        tmp=Path(tmp);raw=tmp/'raw.wav';normalized=tmp/'normalized.wav'
        if settings['backend']=='mambotts':
            params=dict(text=text,text_language='zh',speed=settings['speed'],cut_punc='，。？！；：、…,.;?!')
            if settings.get('reference'):
                params.update(refer_wav_path=settings['reference'],prompt_text=settings['prompt'],prompt_language='zh')
            req=urllib.request.Request(settings['url'].rstrip('/')+'/',
                                       data=json.dumps(params,ensure_ascii=False).encode(),
                                       headers={'Content-Type':'application/json'},method='POST')
            # A local or SSH-forwarded engine must not go through an HTTP proxy.
            opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))
            try:
                with opener.open(req,timeout=600) as response,raw.open('wb') as output:
                    while chunk:=response.read(256*1024):output.write(chunk)
            except urllib.error.HTTPError as e:
                raise RuntimeError(f'MamboTTS HTTP {e.code}: {e.read(400).decode(errors="replace")}') from e
            except urllib.error.URLError as e:
                raise RuntimeError(f'MamboTTS unavailable at {settings["url"]}: {e.reason}') from e
            except (http.client.IncompleteRead,TimeoutError,ConnectionError) as e:
                raise RuntimeError('MamboTTS audio stream was interrupted; inspect the engine log. Existing narration was not replaced.') from e
            with raw.open('rb') as f:head=f.read(12)
            if len(head)<12 or head[:4]!=b'RIFF' or head[8:12]!=b'WAVE':
                raise ValueError('MamboTTS returned non-WAV content; original narration was not replaced')
        else:
            raw=tmp/'raw.aiff';script=tmp/'speech.txt';script.write_text(text)
            subprocess.run(['say','-v',settings['voice'],'-r',str(settings['rate']),
                            '-f',str(script),'-o',str(raw)],check=True)
        subprocess.run(['ffmpeg','-v','error','-xerror','-y','-i',str(raw),'-ar','48000','-ac','1',
                        '-c:a','pcm_s16le',str(normalized)],check=True)
        with wave.open(str(normalized)) as audio:
            if audio.getnframes()==0:raise ValueError('Empty generated audio')
            duration=audio.getnframes()/audio.getframerate()
        normalized.replace(destination)
    meta=dict(request=signature,sha256=digest(destination),seconds=duration)
    tmp_cache=cache.with_suffix('.json.tmp');tmp_cache.write_text(json.dumps(meta,ensure_ascii=False,indent=2)+'\n')
    tmp_cache.replace(cache)
    return True


def add_options(parser):
    parser.add_argument('--speech-backend',choices=['say','mambotts'],default='say')
    parser.add_argument('--mambo-url',default='http://127.0.0.1:9880')
    parser.add_argument('--mambo-speed',type=float,default=1.0)
    parser.add_argument('--voice-revision',default='manbo-e10+manbo_e8_s168')
    parser.add_argument('--mambo-reference',help='Reference WAV path on the engine host; usually use the configured default')
    parser.add_argument('--mambo-prompt',help='Exact reference transcription; required with --mambo-reference')


def settings_for(args,script):
    if args.speech_backend=='say':return dict(backend='say',voice=script['voice'],rate=script['rate'])
    if not .1<=args.mambo_speed<=3:raise ValueError('Mambo speed must be between 0.1 and 3.0')
    if bool(args.mambo_reference)!=bool(args.mambo_prompt):raise ValueError('Reference and its exact transcription must be supplied together')
    return dict(backend='mambotts',url=args.mambo_url,speed=args.mambo_speed,
                revision=args.voice_revision,reference=args.mambo_reference,prompt=args.mambo_prompt)


def description(settings):
    return 'MamboTTS / GPT-SoVITS · 曼波（合成语音）' if settings['backend']=='mambotts' else f'macOS {settings["voice"]}（合成语音）'


if __name__=='__main__':
    p=argparse.ArgumentParser(description='Generate one real narration beat as a voice preview')
    p.add_argument('--output',type=Path,required=True);p.add_argument('--beat',default='opening')
    add_options(p);p.set_defaults(speech_backend='mambotts');args=p.parse_args()
    script=json.loads((Path(__file__).with_name('narration.json')).read_text())
    speech=spoken_text(next(s for key,caption,s in script['beats'] if key==args.beat))
    settings=settings_for(args,script);synthesize(speech,args.output,settings)
    print(json.dumps(dict(file=str(args.output),voice=description(settings),sha256=digest(args.output)),ensure_ascii=False))
