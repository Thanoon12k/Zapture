"""Isolated Linux software-rendering benchmark; requires prepared packages (see docs)."""
import argparse,json,os,pathlib,subprocess,time,threading,queue,statistics,uuid,shlex
import wave
import psutil
import websocket
P=argparse.ArgumentParser();P.add_argument('--root',required=True);P.add_argument('--runs',type=int,default=3);P.add_argument('--seconds',type=int,default=20);P.add_argument('--apps',default='Zapture,OBS');a=P.parse_args()
root=pathlib.Path(a.root).resolve();out=root/'results';out.mkdir(exist_ok=True)
helper=pathlib.Path('/tmp/zapture-obs-mux')
if helper.is_symlink():helper.unlink()
helper.write_text('#!/bin/sh\nexport LD_LIBRARY_PATH='+shlex.quote(str(root/'root/usr/lib/x86_64-linux-gnu'))+'\nexec '+shlex.quote(str(root/'root/usr/lib/x86_64-linux-gnu/obs-plugins/obs-ffmpeg/obs-ffmpeg-mux'))+' \"$@\"\n')
helper.chmod(0o755)
env=os.environ.copy();env.update(DISPLAY='127.0.0.1:98',XDG_SESSION_TYPE='x11',LIBGL_ALWAYS_SOFTWARE='1',LD_LIBRARY_PATH=str(root/'root/usr/lib/x86_64-linux-gnu'),OBS_PLUGINS_PATH=str(root/'root/usr/lib/x86_64-linux-gnu/obs-plugins'),QT_QPA_PLATFORM='xcb',QT_X11_NO_MITSHM='1',XDG_CONFIG_HOME=str(root/'config'))
config=root/'config/obs-studio';(config/'basic/profiles/Benchmark').mkdir(parents=True,exist_ok=True);(config/'plugin_config/obs-websocket').mkdir(parents=True,exist_ok=True)
(config/'global.ini').write_text('[General]\nFirstRun=true\nEnableAutoUpdates=false\n[Basic]\nProfile=Benchmark\nProfileDir=Benchmark\nSceneCollection=Benchmark\nSceneCollectionFile=Benchmark\n[BasicWindow]\nPreviewEnabled=false\n')
(config/'plugin_config/obs-websocket/config.json').write_text(json.dumps({'server_enabled':True,'server_port':4457,'auth_required':False,'alerts_enabled':False,'first_load':False}))
(config/'basic/profiles/Benchmark/basic.ini').write_text(f'''[General]
Name=Benchmark
[Video]
BaseCX=1280
BaseCY=720
OutputCX=1280
OutputCY=720
FPSType=0
FPSCommon=30
[Output]
Mode=Advanced
[AdvOut]
RecType=Standard
RecFilePath={out}
RecFormat2=mkv
RecEncoder=obs_x264
RecTracks=1
TrackIndex=1
[Audio]
SampleRate=48000
ChannelSetup=Stereo
''')
(config/'basic/profiles/Benchmark/recordEncoder.json').write_text(json.dumps({'rate_control':'CRF','crf':23,'preset':'ultrafast','profile':'baseline','keyint_sec':0,'x264opts':''}))
silence=root/'silence.wav'
with wave.open(str(silence),'wb') as wav:
 wav.setnchannels(2);wav.setsampwidth(2);wav.setframerate(48000);wav.writeframes(bytes(48000*4*10))
workload=root/'workload.py';workload.write_text('''import tkinter as t,time,math
r=t.Tk();r.geometry('1280x720+0+0');r.overrideredirect(True)
c=t.Canvas(r,width=1280,height=720,bg='#1c2938',highlightthickness=0);c.pack()
for i in range(12):c.create_rectangle(i*110,0,i*110+55,720,fill=['#31465c','#3a546b'][i%2],outline='')
box=c.create_rectangle(0,200,180,380,fill='#ff6348',outline='')
text=c.create_text(640,90,text='Zapture / OBS synthetic benchmark - 720p30',fill='white',font=('sans',28))
start=time.monotonic()
def frame():
 x=(time.monotonic()-start)*180%1100;c.coords(box,x,240,x+180,420);r.after(33,frame)
frame();r.mainloop()
''')
def log(name):return open(out/name,'w')
def stop(p):
 if p and p.poll() is None:
  p.terminate()
  try:p.wait(timeout=8)
  except subprocess.TimeoutExpired:p.kill();p.wait()
def sample(p,seconds):
 data=[];prev={};start=time.monotonic();last=start
 while time.monotonic()-start<seconds:
  if p.poll() is not None:raise RuntimeError('Application exited during measurement')
  cpu=0;rss=0;pss=0;now=time.monotonic();seen={}
  try:processes=[psutil.Process(p.pid)]+psutil.Process(p.pid).children(recursive=True)
  except psutil.Error:processes=[]
  for q in processes:
   try:
    key=(q.pid,q.create_time());ct=q.cpu_times();value=ct.user+ct.system;seen[key]=value
    if key in prev:cpu+=max(0,value-prev[key])
    rss+=q.memory_info().rss
    try:pss+=q.memory_full_info().pss
    except (psutil.Error,AttributeError):pss=0
   except psutil.Error:pass
  if prev:data.append({'elapsed':now-start,'cpu_core_pct':cpu/(now-last)*100,'rss_mib':rss/1048576,'pss_mib':pss/1048576 if pss else None})
  prev=seen;last=now;time.sleep(.5)
 return {'samples':data,'cpu_core_pct_mean':statistics.mean(d['cpu_core_pct'] for d in data),'rss_mib_median':statistics.median(d['rss_mib'] for d in data),'pss_mib_median':statistics.median(d['pss_mib'] for d in data if d['pss_mib'] is not None) if any(d['pss_mib'] for d in data) else None}
class MCP:
 def __init__(self,p):
  self.p=p;self.q=queue.Queue();self.id=0
  def read():
   for line in p.stdout:
    try:self.q.put(json.loads(line))
    except ValueError:pass
  threading.Thread(target=read,daemon=True).start()
 def call(self,method,params):
  self.id+=1;self.p.stdin.write(json.dumps({'jsonrpc':'2.0','id':self.id,'method':method,'params':params})+'\n');self.p.stdin.flush();end=time.monotonic()+20
  while time.monotonic()<end:
   r=self.q.get(timeout=max(.1,end-time.monotonic()))
   if r.get('id')==self.id:
    if 'error' in r:raise RuntimeError(str(r['error']))
    result=r['result']
    if result.get('isError'):raise RuntimeError(str(result))
    return result
  raise TimeoutError('MCP response')
 def tool(self,name):
  r=self.call('tools/call',{'name':name,'arguments':{'audio':'off'} if name=='zapture_start_recording' else {}})
  if 'structuredContent' in r:return r['structuredContent']
  for c in r.get('content',[]):
   if c.get('type')=='text':
    try:return json.loads(c['text'])
    except ValueError:pass
  return r
class OBS:
 def __init__(self):
  deadline=time.monotonic()+30
  while True:
   try:self.w=websocket.create_connection('ws://127.0.0.1:4457',timeout=4);break
   except Exception:
    if time.monotonic()>deadline:raise
    time.sleep(.5)
  self.w.recv();self.w.send(json.dumps({'op':1,'d':{'rpcVersion':1,'eventSubscriptions':0}}));self.w.recv()
  ready_deadline=time.monotonic()+25
  while True:
   try:self.call('GetSceneList');break
   except RuntimeError as error:
    if '207' not in str(error) or time.monotonic()>ready_deadline:raise
    time.sleep(.5)
 def call(self,name,data=None):
  uid=uuid.uuid4().hex;self.w.send(json.dumps({'op':6,'d':{'requestType':name,'requestId':uid,'requestData':data or {}}}))
  while True:
   r=json.loads(self.w.recv())
   if r['op']==7 and r['d']['requestId']==uid:
    if not r['d']['requestStatus']['result']:raise RuntimeError(str(r['d']['requestStatus']))
    return r['d'].get('responseData',{})
def validate(path):
 p=pathlib.Path(path)
 r=subprocess.run(['ffprobe','-v','error','-count_frames','-select_streams','v:0','-show_entries','stream=width,height,nb_read_frames,avg_frame_rate,duration:format=duration,size','-of','json',str(p)],capture_output=True,text=True,timeout=30)
 if r.returncode:raise RuntimeError(r.stderr)
 result=json.loads(r.stdout)
 # Decode representative frames and compare their bytes; an existing file alone is not success.
 frames=[]
 for sec in [2,7,12]:
  raw=subprocess.run(['ffmpeg','-v','error','-ss',str(sec),'-i',str(p),'-frames:v','1','-vf','scale=160:90','-f','rawvideo','-pix_fmt','rgb24','-'],capture_output=True,timeout=20)
  if raw.returncode or len(raw.stdout)!=43200:raise RuntimeError('Frame decode failed')
  frames.append(raw.stdout)
 result['motion_detected']=len(set(frames))>1;result['path']=str(p);result['bytes']=p.stat().st_size
 if not result['motion_detected']:raise RuntimeError('No changing visual content in sampled frames')
 return result
def run_benchmark():
 x=None;scene=None;app=None;results=[]
 try:
  x=subprocess.Popen([str(root/'root/usr/bin/Xvfb'),':98','-screen','0','1280x720x24','-nolisten','unix','-nolisten','local','-listen','tcp','-ac'],stdout=log('xvfb.log'),stderr=subprocess.STDOUT);time.sleep(2)
  scene=subprocess.Popen(['python3',str(workload)],env=env,stdout=log('workload.log'),stderr=subprocess.STDOUT);time.sleep(2)
  hardware={'platform':os.uname()._asdict() if hasattr(os.uname(),'_asdict') else str(os.uname()),'logical_cpus':psutil.cpu_count(),'memory_bytes':psutil.virtual_memory().total,'cpuinfo':pathlib.Path('/proc/cpuinfo').read_text()[:1500],'renderer':'Mesa llvmpipe; no GPU device','resolution':[1280,720],'fps':30,'encoder':'x264 ultrafast CRF23','audio':'off (OBS may mux silent AAC track)','seconds':a.seconds,'runs':a.runs,'versions':{'zapture':'0.1.0-alpha.2','obs':'30.2.3 Debian package'},'notes':['Cloud container, virtual X11, software OpenGL; not physical Linux or Windows benchmarks','Resource samples include child processes, especially Zapture FFmpeg','Startup latency uses different API readiness criteria; not a direct launch-speed comparison','OBS preview disabled; Zapture floating controls may appear in capture','CPU percentage: 100% = one logical core; RSS sum may double-count shared pages']}
  (out/'environment.json').write_text(json.dumps(hardware,indent=2))
  for trial in range(1,a.runs+1):
   for name in (['Zapture','OBS'] if trial%2 else ['OBS','Zapture']):
    if name not in a.apps.split(','):continue
    row={'app':name,'trial':trial};print('START',name,trial,flush=True)
    try:
     begin=time.monotonic()
     if name=='Zapture':
      app=subprocess.Popen([str(next((root/'apps').rglob('zapture'))),'--mcp'],env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=log(f'zapture-{trial}.log'),text=True)
      control=MCP(app);control.call('initialize',{'protocolVersion':'2025-06-18','capabilities':{},'clientInfo':{'name':'benchmark','version':'1'}})
      control.tool('zapture_status')
     else:
      app=subprocess.Popen([str(root/'root/usr/bin/obs'),'--disable-updater','--disable-missing-files-check','--profile','Benchmark','--collection','Benchmark'],env=env,stdout=log(f'obs-{trial}.log'),stderr=subprocess.STDOUT,cwd=root/'root/usr/bin')
      control=OBS();row['obs_version']=control.call('GetVersion')
      try:control.call('CreateScene',{'sceneName':'Benchmark'})
      except RuntimeError:pass
      inputs=control.call('GetInputList')['inputs']
      if not any(i['inputName']=='Screen' for i in inputs):control.call('CreateInput',{'sceneName':'Benchmark','inputName':'Screen','inputKind':'xshm_input','inputSettings':{'screen':0,'show_cursor':False},'sceneItemEnabled':True})
      if not any(i['inputName']=='Silent timing source' for i in inputs):control.call('CreateInput',{'sceneName':'Benchmark','inputName':'Silent timing source','inputKind':'ffmpeg_source','inputSettings':{'is_local_file':True,'local_file':str(silence),'looping':True},'sceneItemEnabled':True})
      control.call('SetCurrentProgramScene',{'sceneName':'Benchmark'})
     row['api_ready_seconds']=time.monotonic()-begin;time.sleep(5)
     row['idle']=sample(app,10)
     if name=='Zapture':row['start_response']=control.tool('zapture_start_recording')
     else:control.call('StartRecord')
     time.sleep(2)
     if name=='Zapture':
      st=control.tool('zapture_status')
      if st.get('state')!='recording':raise RuntimeError('Capture did not remain active: '+str(st))
     row['recording']=sample(app,a.seconds)
     finish=time.monotonic()
     if name=='Zapture':
      control.tool('zapture_stop_recording')
      for _ in range(60):
       st=control.tool('zapture_status')
       if st.get('state')=='idle':break
       time.sleep(.2)
      if st.get('state')!='idle' or not st.get('message','').startswith('Saved:'):raise RuntimeError('Finalization failed: '+str(st))
      path=st['recording_path']
     else:
      row['obs_stats']=control.call('GetStats');path=control.call('StopRecord')['outputPath']
      deadline=time.monotonic()+30
      while time.monotonic()<deadline:
       if not control.call('GetRecordStatus')['outputActive'] and pathlib.Path(path).exists():break
       time.sleep(.25)
     row['finalize_seconds']=time.monotonic()-finish;row['output']=validate(path);row['status']='ok'
    except Exception as e:row['status']='failed';row['error']=str(e);print('FAIL',name,str(e),flush=True)
    finally:
     if name=='OBS' and 'control' in locals() and hasattr(control,'w'):control.w.close()
     stop(app);app=None
    results.append(row);(out/'raw-results.json').write_text(json.dumps(results,indent=2));print('DONE',name,trial,row['status'],flush=True);time.sleep(2)
 finally:
  stop(app);stop(scene);stop(x)

if __name__ == "__main__":
 run_benchmark()
