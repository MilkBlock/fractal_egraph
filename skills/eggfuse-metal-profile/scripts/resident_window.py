#!/usr/bin/env python3
"""Live counters or bounded capture within the unchanged full resident schedule.

Pause only the host at outer submission boundaries to attach/detach profiling.
No --stop-after-runs: private multi-stage execution remains enabled. The complete
endpoint and history are checked against the saved same-worker control.
"""
import argparse,json,os,signal,subprocess,time
from pathlib import Path
from profile import Notification,issues,export
from resident_full import sha,replace,verify
from report import summarize as timeline_summary
from counters import summarize as counter_summary
from artifacts import capture_filesystem,mirror_summaries


def instrument(text,start,count,mode,windows=None):
    windows=windows or [(start,count)]
    hook=r'''
let pwRoot=URL(fileURLWithPath:ProcessInfo.processInfo.environment["EGGFUSE_PROFILE_WINDOW"]!)
let pwWindows:[[Int]]=__WINDOWS__
var pwState=0,pwStart=0,pwIndex=0
var pwFolder:URL {pwRoot.appendingPathComponent("window-\(pwIndex)")}
func pwWrite(_ name:String){
 let info:[String:Any] = ["worker_pid":ProcessInfo.processInfo.processIdentifier,"outer_command":dispatches,"completed_runs":(0..<n).map{u32(states,$0*stateWords+3,1)[0]},"pc":u32(states,0,1)[0],"logical_boundary":atBoundary(0)]
 try! JSONSerialization.data(withJSONObject:info,options:[.sortedKeys]).write(to:pwFolder.appendingPathComponent(name),options:.atomic)
}
func pwWait(_ file:String){
 while !FileManager.default.fileExists(atPath:pwFolder.appendingPathComponent(file).path){Thread.sleep(forTimeInterval:0.02)}
}
'''
    hook=hook.replace('__WINDOWS__',json.dumps(windows))
    text=replace(text,'let start=CFAbsoluteTimeGetCurrent()',hook+'\nlet start=CFAbsoluteTimeGetCurrent()')
    begin='''
 if pwIndex<pwWindows.count && pwState==0 && dispatches>=pwWindows[pwIndex][0] {
  pwStart=dispatches;pwWrite("window-start.json")
  __BEGIN__
  pwState=1
 }
 if pwState==1 && dispatches>=pwStart+pwWindows[pwIndex][1] {
  __END__
  pwWrite("window-end.json");pwIndex+=1;pwState=pwIndex==pwWindows.count ? 2:0
 }
'''.replace('__START__',str(start)).replace('__COUNT__',str(count))
    if mode=='capture':
        begin=begin.replace('__BEGIN__','''let d=MTLCaptureDescriptor();d.captureObject=queue;d.destination = .gpuTraceDocument;d.outputURL=pwFolder.appendingPathComponent("window.gputrace")
  try require(MTLCaptureManager.shared().supportsDestination(.gpuTraceDocument),"capture unavailable")
  try MTLCaptureManager.shared().startCapture(with:d)''').replace('__END__','MTLCaptureManager.shared().stopCapture()')
        text=replace(text,'if capturing && phaseStarted {e.endEncoding();e=cb.makeComputeCommandEncoder()!;bind(e)}','if (capturing || pwState==1) && phaseStarted {e.endEncoding();e=cb.makeComputeCommandEncoder()!;bind(e)}')
        text=replace(text,'if capturing {e.label="fn=\\(name) resident"}','if capturing || pwState==1 {e.label="fn=\\(name) resident"}')
    else:
        begin=begin.replace('__BEGIN__','pwWait("release-start")').replace('__END__','pwWrite("window-gpu-done.json");pwWait("release-end")')
        text=replace(text,'if capturing {e.label="fn=\\(name) resident"}','if capturing {e.label="fn=\\(name) resident"}\n    else if pwState==1 && !phaseStarted {e.label="fn=resident_pipeline outer=\\(dispatches)"}')
    text=replace(text,'var lastProgress=start\nwhile true {','var lastProgress=start\nwhile true {\n'+begin)
    text=replace(text,'"profile_capture":capturing,','"profile_capture":capturing,"profile_window_state":pwState,"profile_window_count":pwIndex,')
    return text


def wait_file(path,worker):
    while not path.exists():
        if worker.poll() is not None:raise RuntimeError('worker ended before '+path.name)
        time.sleep(.05)
    return json.loads(path.read_text())


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--run',type=Path,required=True);p.add_argument('--build',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    p.add_argument('--start-command',type=int,help='outer command-buffer index, not logical Run');p.add_argument('--commands',type=int,default=1);p.add_argument('--window',action='append',help='repeatable START:COUNT windows in one full execution')
    p.add_argument('--mode',choices=['counters','capture'],default='counters');p.add_argument('--template',default='Cell Metal Limiters');a=p.parse_args()
    if a.window:
        if a.start_command is not None:p.error('choose --window or --start-command')
        try:windows=[tuple(map(int,x.split(':'))) for x in a.window]
        except ValueError:p.error('window must be START:COUNT')
    elif a.start_command is not None:windows=[(a.start_command,a.commands)]
    else:p.error('--window or --start-command required')
    if any(len(w)!=2 or w[0]<0 or w[1]<1 for w in windows):p.error('positive command windows required')
    if any(b[0]<a[0]+a[1] for a,b in zip(windows,windows[1:])):p.error('windows must be sorted and non-overlapping')
    run=a.run.resolve();build=a.build.resolve();out=a.output.resolve();reference=json.loads((run/'result.json').read_text());base=build/'generic-cell-worker';source=build/'generic-cell-worker.swift'
    if sha(base)!=reference['worker_sha256'] or sha(source)!=(build/'generic-cell-worker.sha256').read_text():raise ValueError('base worker/source differs from saved control')
    if any(start+count>reference['dispatches'] for start,count in windows):raise ValueError('window exceeds saved complete execution')
    if a.mode=='capture':capture_filesystem(out)
    out.mkdir(parents=True,exist_ok=False);(out/'tmp').mkdir()
    for i in range(len(windows)):(out/f'window-{i}').mkdir()
    src=out/'worker.swift';src.write_text(instrument(source.read_text(),0,1,a.mode,windows));image=out/'worker'
    subprocess.run(['xcrun','swiftc','-O',str(src),'-o',str(image)],check=True)
    provenance=dict(mode=a.mode,source_program=str(run/'program.json'),source_program_sha256=sha(run/'program.json'),reference=str(run/'result.json'),reference_sha256=sha(run/'result.json'),base_worker_sha256=sha(base),profile_worker_sha256=sha(image),profile_source_sha256=sha(src),windows=windows,xcode=subprocess.check_output(['xcodebuild','-version'],text=True).strip(),scope='full original schedule, host-gated diagnostic window; no clean benchmark time')
    (out/'provenance.json').write_text(json.dumps(provenance,indent=2)+'\n')
    mirror_summaries(out)
    env={**os.environ,'TMPDIR':str(out/'tmp')+'/','EGGFUSE_PROFILE_WINDOW':str(out),'EGGFUSE_METAL_CACHE':str(build/'metal-cache')}
    if a.mode=='capture':env['METAL_CAPTURE_ENABLED']='1'
    worker=trace=notification=None
    try:
        with (out/'worker.log').open('w') as log,(out/'xctrace.log').open('w') as trace_log:
            worker=subprocess.Popen([str(image),str(run/'program.json'),str(out/'result.json')],env=env,stdout=log,stderr=subprocess.STDOUT)
            replay_windows=[]
            for i,_ in enumerate(windows):
                window=out/f'window-{i}'
                with (window/'xctrace.log').open('w') as window_log:
                    start=wait_file(window/'window-start.json',worker);print('window ready',start,flush=True)
                    if a.mode=='counters':
                        notification=Notification();settings=json.loads(subprocess.check_output(['xcrun','xctrace','record','--template',a.template,'--show-recording-options'],text=True))
                        if 'Time Profiler' not in settings:raise ValueError('template lacks Time Profiler for CPU waits')
                        settings['Time Profiler']['recordWaitingThreads']=True;(window/'recording-options.json').write_text(json.dumps(settings))
                        cmd=['xcrun','xctrace','record','--attach',str(worker.pid),'--output',str(window/'profile.trace'),'--no-prompt','--notify-tracing-started',notification.name,'--template',a.template,'--recording-options',str(window/'recording-options.json')]
                        (window/'xctrace-command.json').write_text(json.dumps(cmd,indent=2)+'\n');trace=subprocess.Popen(cmd,env=env,stdout=window_log,stderr=subprocess.STDOUT)
                        until=time.monotonic()+60
                        while not notification.ready():
                            if trace.poll() is not None or time.monotonic()>until:raise RuntimeError('xctrace did not become ready; inspect xctrace.log')
                            time.sleep(.05)
                        (window/'release-start').touch();end=wait_file(window/'window-gpu-done.json',worker)
                        if trace.poll() is None:trace.send_signal(signal.SIGINT)
                        trace.wait(timeout=240)
                        if trace.returncode:raise RuntimeError('xctrace failed; do not use partial counters')
                        (window/'release-end').touch()
                    else:end=wait_file(window/'window-end.json',worker)
                    replay_windows.append(dict(start=start,end=end))
                    if notification:notification.close();notification=None
            worker.wait()
            if worker.returncode:raise RuntimeError('profiled worker failed')
        actual=json.loads((out/'result.json').read_text());check=verify(actual,reference)
        if actual.get('profile_window_count')!=len(windows):raise ValueError('not every requested window completed')
        replay=dict(worker_pid=worker.pid,saved_run_metadata_equal=True,full_state_and_history_equal=True,verification=check,windows=replay_windows,diagnostic_only=True)
        (out/'replay.json').write_text(json.dumps(replay,indent=2)+'\n')
        mirror_summaries(out)
        if a.mode=='counters':
            from resident_report import analyze
            summaries=[]
            for i,_ in enumerate(windows):
                d=analyze(out/f'window-{i}',worker.pid);d['coordinates']=replay_windows[i];summaries.append(d)
                mirror_summaries(out)
                print('window',i,'counters',d['counters']['rows'],'samples; GPU active',d['timeline']['gpu_active_seconds'],flush=True)
            (out/'hardware-summary.json').write_text(json.dumps(dict(schema='eggfuse-resident-hardware-windows/v1',verification=check,windows=summaries),indent=2)+'\n')
        mirror_summaries(out)
        print('complete state verified',out,flush=True)
    except BaseException as e:
        (out/'failure.json').write_text(json.dumps(dict(error=str(e),diagnostic_only=True),indent=2)+'\n');raise
    finally:
        if worker and worker.poll() is None:worker.terminate();worker.wait(timeout=30)
        if trace and trace.poll() is None:trace.send_signal(signal.SIGINT);trace.wait(timeout=240)
        if notification:notification.close()
if __name__=='__main__':main()
