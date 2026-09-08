#!/usr/bin/env python3
"""Measure a retained synthetic 10,002-event trace in an isolated native app."""
import json,pathlib,time,urllib.request,subprocess,threading,ctypes
ROOT=pathlib.Path(__file__).resolve().parents[1]/'artifacts/trace-research-e2e/scale-native-store'
cfg=json.loads((ROOT/'visuals-ipc.json').read_text())
def call(path,body=None):
    req=urllib.request.Request(cfg['url']+path,data=None if body is None else json.dumps(body).encode(),headers={'Authorization':'Bearer '+cfg['token'],'Content-Type':'application/json'})
    try:
        with urllib.request.urlopen(req,timeout=120) as response:return json.load(response)
    except urllib.error.HTTPError as error:
        raise RuntimeError(error.read().decode()) from None
packet=json.loads((ROOT/'window-first.json').read_text())
digest=packet['trace_digest']
trace=next(row for row in call('/v1/traces')['traces'] if row.get('traceId')==packet['trace_id'] or row.get('digest')==digest)
# Match WebKit helpers by the kernel resource coalition, not by process name.
# ABI: https://github.com/apple/darwin-xnu/blob/main/bsd/sys/proc_info.h
libproc=ctypes.CDLL('/usr/lib/libproc.dylib',use_errno=True)
libproc.proc_pidinfo.argtypes=[ctypes.c_int,ctypes.c_int,ctypes.c_uint64,ctypes.c_void_p,ctypes.c_int]
def coalition(pid):
    buf=(ctypes.c_uint64*5)()
    size=libproc.proc_pidinfo(pid,20,0,buf,ctypes.sizeof(buf))
    return int(buf[0]) if size==40 and buf[0]>0 else None
samples=[];done=threading.Event()
def sample_memory():
    while not done.is_set():
        rows=[]
        for row in subprocess.check_output(['ps','-axo','pid=,rss=,comm='],text=True).splitlines():
            pid,rss,comm=row.strip().split(None,2);rows.append((int(pid),int(rss),comm))
        host=next((r for r in rows if 'Workshop Trace Acceptance.app/Contents/MacOS/synth-desktop' in r[2]),None)
        if host:
            identity=coalition(host[0])
            if identity:
                members=[{'pid':p,'rssKiB':r,'executable':c} for p,r,c in rows if coalition(p)==identity]
                samples.append({'pid':host[0],'rssKiB':host[1],'coalitionId':identity,'coalitionRssKiB':sum(m['rssKiB'] for m in members),'members':members})
        done.wait(.1)
thread=threading.Thread(target=sample_memory,daemon=True);thread.start()
started=time.monotonic()
visual=call('/v1/traces/open',{'trace_id':trace['traceId']})
call('/v1/review-window/capture',{'visualId':visual['visualId'],'width':1200,'height':800,'outputPath':str(ROOT/'native-window-loading.png')})
while time.monotonic()-started<90:
    observation=call('/v1/review-observations/'+visual['visualId']).get('observation')
    if observation and observation.get('semanticEventCount',0)>0:break
    time.sleep(.5)
else:raise RuntimeError('Native window never became usable')
elapsed=round((time.monotonic()-started)*1000)
call('/v1/review-window/capture',{'visualId':visual['visualId'],'width':1200,'height':800,'outputPath':str(ROOT/'native-window.png')})
done.set();thread.join(2)
(ROOT/'native-window-acceptance.json').write_text(json.dumps({'status':'rendered','traceDigest':digest,'visualId':visual['visualId'],'firstUsableNativeMs':elapsed,'observation':observation,'fixture':'retained synthetic 10002 events','includes':'IPC open, projection, transport, native render, capture/poll overhead','nextWindowClickVerified':False,'hostMemory':{'scope':'isolated native host RSS; excludes WebKit XPC helpers','samples':len(samples),'baselineKiB':samples[0]['rssKiB'] if samples else None,'peakKiB':max((s['rssKiB'] for s in samples),default=None)},'appMemory':{'scope':'sum of RSS for the isolated host resource coalition, including WebKit helpers; shared pages may be counted multiple times','samples':len(samples),'baselineKiB':samples[0]['coalitionRssKiB'] if samples else None,'peakKiB':max((s['coalitionRssKiB'] for s in samples),default=None),'peakMembers':max(samples,key=lambda s:s['coalitionRssKiB'])['members'] if samples else []}},indent=2))
print(json.dumps({'visualId':visual['visualId'],'firstUsableNativeMs':elapsed,'semanticEventCount':observation.get('semanticEventCount')}))
