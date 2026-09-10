#!/usr/bin/env python3
"""Local query/media service plus capability-scoped append-only evidence writes."""
import html, json, mimetypes, re, subprocess, threading
FRAME_LOCK=threading.Lock()
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
from urllib.parse import urlparse,parse_qs,unquote
from query import OUT,HERE,query,QUERIES
from evidence_api import capability, evidence
class Handler(BaseHTTPRequestHandler):
 def cors(self):
  origin=self.headers.get('Origin','')
  if origin in ('http://127.0.0.1:8128','http://localhost:8128','http://127.0.0.1:14338','http://localhost:14338','tauri://localhost','http://tauri.localhost','https://tauri.localhost'):
   self.send_header('Access-Control-Allow-Origin',origin);self.send_header('Vary','Origin')
 def do_OPTIONS(self):
  self.send_response(204);self.cors();self.send_header('Access-Control-Allow-Headers','Content-Type, X-Annotation-Capability');self.send_header('Access-Control-Allow-Methods','GET, POST, OPTIONS');self.end_headers()
 def do_POST(self):
  try:
   u=urlparse(self.path)
   if u.path!='/annotations':return self.send_error(404)
   if not __import__('hmac').compare_digest(self.headers.get('X-Annotation-Capability',''),capability()):return self.json({'error':'Annotation capability required'},403)
   length=int(self.headers.get('Content-Length','0'))
   if not 0<length<=24000:raise ValueError('Invalid annotation request size')
   return self.json(evidence(parse_qs(u.query).get('run',[''])[0],json.loads(self.rfile.read(length))))
  except Exception as e:self.json({'error':str(e)},400)
 def log_message(self,*args):pass
 def do_GET(self):
  u=urlparse(self.path)
  try:
   if u.path=='/viewer':
    b=(HERE/'web/index.html').read_bytes();self.send_response(200);self.send_header('Content-Type','text/html; charset=utf-8');self.send_header('Content-Length',str(len(b)));self.end_headers();self.wfile.write(b);return
   if u.path=='/health':return self.json({'ready':True})
   if u.path=='/annotations':return self.json(evidence(parse_qs(u.query).get('run',[''])[0]))
   if u.path=='/query':
    q=parse_qs(u.query);return self.json(query(q.get('sql',[QUERIES[q.get('name',['all'])[0]]])[0]))
   if u.path=='/evidence':
    q=parse_qs(u.query);run=q.get('run',[''])[0];scope=q.get('scope',[''])[0];line=int(q.get('line',['0'])[0])
    if not re.fullmatch(r'[a-zA-Z0-9-]+',run) or scope not in ['run','episode','engine'] or line<1 or line>1000000:raise ValueError('Invalid evidence reference')
    p=OUT/run/({'episode':'episode/events.jsonl','run':'events.jsonl','engine':'episode/engine-states.jsonl'}[scope])
    if not p.resolve().is_relative_to(OUT.resolve()) or not p.is_file():return self.send_error(404)
    with p.open() as f:
     for n,row in enumerate(f,1):
      if n==line:
       record={'source':str(p.relative_to(OUT)),'line':line,'record':json.loads(row)}
       if 'text/html' not in self.headers.get('Accept',''):return self.json(record)
       content=html.escape(json.dumps(record,indent=2))
       b=('<!doctype html><meta charset="utf-8"><title>RuneBench source record</title><style>body{font:14px system-ui;margin:24px}pre{white-space:pre-wrap;overflow-wrap:anywhere}</style><h1>Exact source record</h1><pre>'+content+'</pre>').encode()
       self.send_response(200);self.send_header('Content-Type','text/html; charset=utf-8');self.send_header('Content-Length',str(len(b)));self.end_headers();self.wfile.write(b);return
    return self.send_error(404)
   if u.path=='/data':return self.json(json.loads((HERE/'swarm-data.json').read_text()))
   if u.path.startswith('/frame/'):
    parts=unquote(u.path).split('/');q=parse_qs(u.query)
    if len(parts)!=4 or not re.fullmatch(r'[a-zA-Z0-9-]+',parts[2]) or not re.fullmatch(r'[a-z0-9]+\.jpg',parts[3]):raise ValueError('Invalid frame path')
    ms=int(q.get('ms',['0'])[0])
    if not 0<=ms<=600000:raise ValueError('Invalid frame timestamp')
    ms=ms//500*500;actor=parts[3][:-4];p=OUT/parts[2]/'episode'/f'{actor}.mp4'
    if not p.is_file():return self.send_error(404)
    cache=HERE/'frame-cache'/parts[2];cache.mkdir(parents=True,exist_ok=True);frame=cache/f'{actor}-{ms}.jpg'
    with FRAME_LOCK:
     if not frame.exists():
      result=subprocess.run(['ffmpeg','-loglevel','error','-ss',str(ms/1000),'-i',str(p),'-frames:v','1','-q:v','3','-f','image2pipe','-vcodec','mjpeg','-'],capture_output=True,timeout=15)
      if result.returncode or not result.stdout:raise ValueError('Frame unavailable at selected timestamp')
      frame.write_bytes(result.stdout)
    b=frame.read_bytes();self.send_response(200);self.cors();self.send_header('Content-Type','image/jpeg');self.send_header('Content-Length',str(len(b)));self.end_headers();self.wfile.write(b);return
   if u.path.startswith('/media/'):
    parts=unquote(u.path).split('/')
    if len(parts)!=4 or not re.fullmatch(r'[a-zA-Z0-9-]+',parts[2]) or not re.fullmatch(r'[a-z0-9]+\.mp4',parts[3]):raise ValueError('Invalid media path')
    p=(OUT/parts[2]/'episode'/parts[3]).resolve()
    if not p.is_relative_to(OUT.resolve()) or not p.is_file():return self.send_error(404)
    size=p.stat().st_size;start=0;end=size-1;partial=False
    if self.headers.get('Range'):
     m=re.fullmatch(r'bytes=(\d+)-(\d*)',self.headers['Range'])
     if not m:return self.send_error(416)
     start=int(m[1]);end=min(end,int(m[2]) if m[2] else end);partial=True
     if start>end:return self.send_error(416)
    self.send_response(206 if partial else 200);self.send_header('Content-Type','video/mp4');self.send_header('Accept-Ranges','bytes');self.send_header('Content-Length',str(end-start+1))
    if partial:self.send_header('Content-Range',f'bytes {start}-{end}/{size}')
    self.end_headers()
    with p.open('rb') as f:
     f.seek(start);left=end-start+1
     while left>0:
      b=f.read(min(left,65536));self.wfile.write(b);left-=len(b)
    return
   self.send_error(404)
  except (BrokenPipeError,ConnectionResetError):pass
  except Exception as e:self.json({'error':str(e)},400)
 def json(self,data,status=200):
  b=json.dumps(data).encode();self.send_response(status);self.cors();self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(b)));self.send_header('Cache-Control','no-store');self.end_headers();self.wfile.write(b)
if __name__=='__main__':ThreadingHTTPServer(('127.0.0.1',8128),Handler).serve_forever()
