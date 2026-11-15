"""54 scenarios, three repeats: synthetic Linux capture comparison.
Requires linux_cloud.py setup plus scrot/libimlib2t64 and ImageMagick.
"""
import json,time,subprocess,pathlib,threading,random,hashlib,statistics
import psutil
from PIL import Image,ImageStat
import linux_cloud as b
out=b.root/'matrix-results';out.mkdir(exist_ok=True)
b.env['IMLIB2_LOADER_PATH']=str(b.root/'root/usr/lib/x86_64-linux-gnu/imlib2/loaders')
rows=[];rng=random.Random(241115)
def save(): (out/'results.json').write_text(json.dumps(rows,separators=(',',':')))
def tool(name,args=None):
 r=control.call('tools/call',{'name':name,'arguments':args or {}})
 return json.loads(next(c['text'] for c in r['content'] if c['type']=='text'))
def finish():
 t=time.monotonic();tool('zapture_stop_recording')
 for _ in range(150):
  st=tool('zapture_status')
  if st['state']=='idle':
   assert st.get('message','').startswith('Saved:'),st
   return st['recording_path'],time.monotonic()-t
  time.sleep(.1)
 raise TimeoutError('Stop timeout')
def probe(path,w,h):
 d=json.loads(subprocess.check_output(['ffprobe','-v','error','-count_frames','-select_streams','v:0','-show_entries','stream=width,height,nb_read_frames,avg_frame_rate:format=duration,size','-of','json',str(path)],timeout=30))
 s=d['streams'][0];assert (s['width'],s['height'])==(w,h)
 assert int(s['nb_read_frames'])>10
 raw=subprocess.check_output(['ffmpeg','-v','error','-ss','2','-i',str(path),'-frames:v','1','-vf','scale=160:90','-pix_fmt','rgb24','-f','rawvideo','-'],timeout=30)
 assert len(raw)==43200 and max(raw)-min(raw)>30
 d['decoded_frame_sha256']=hashlib.sha256(raw).hexdigest();d['delivered_fps']=int(s['nb_read_frames'])/float(d['format']['duration']);return d
class Peak:
 def __init__(self,p):self.p=p;self.done=False;self.samples=[];self.t=threading.Thread(target=self.run);self.t.start()
 def run(self):
  while not self.done:
   try:
    root=psutil.Process(self.p.pid);rss=sum(q.memory_info().rss for q in [root]+root.children(recursive=True));self.samples.append(rss/1048576)
   except psutil.Error:pass
   time.sleep(.01)
 def end(self):self.done=True;self.t.join();return max(self.samples) if self.samples else None
scene_source='''import tkinter as t,time,math,sys
w,h,mode=int(sys.argv[1]),int(sys.argv[2]),sys.argv[3]
r=t.Tk();r.geometry(f'{w}x{h}+0+0');r.overrideredirect(True)
c=t.Canvas(r,width=w,height=h,bg='#17273b',highlightthickness=0);c.pack()
c.create_rectangle(0,0,w,h*.13,fill='#e7eee4',outline='');c.create_text(w*.05,h*.065,anchor='w',text='SYNTHETIC CAPTURE TEST / '+mode,fill='#243e52',font=('sans',int(h*.025)))
for row in range(16):
 c.create_rectangle(w*.05,h*(.18+row*.045),w*(.75+(row%4)*.05),h*(.19+row*.045),fill=['#486783','#688aa5'][row%2],outline='')
n=0 if mode=='document' else (8 if mode=='animation' else 160)
items=[c.create_rectangle(0,0,w*.025,h*.035,fill=['#e95a42','#83c09a','#dfb969','#649dd2'][i%4],outline='') for i in range(n)]
start=time.monotonic()
def tick():
 now=time.monotonic()-start
 for i,item in enumerate(items):
  x=((now*(.13+(i%5)*.025)+i*.071)%1)*w;y=h*(.17+((math.sin(now+i*.6)+1)/2)*.77)
  c.coords(item,x,y,x+w*.025,y+h*.035)
 r.lift();r.after(33,tick)
tick();r.mainloop()
'''
(out/'versions.json').write_text(json.dumps({'zapture':'0.1.0-alpha.2 published Linux x64','ffmpeg':subprocess.check_output(['ffmpeg','-version'],text=True).splitlines()[0],'imagemagick':subprocess.check_output(['import','-version'],text=True).splitlines()[0],'scrot':subprocess.check_output([str(b.root/'root/usr/bin/scrot'),'--version'],env=b.env,text=True).strip(),'seed':241115,'repeats':3,'recording_sample_seconds':b.a.seconds,'recording_warmup_seconds':2,'note':'Same software-only cloud environment as earlier benchmark; synthetic content; 30fps; no audio'},indent=2))
for w,h in [(1280,720),(1920,1080),(2560,1440)]:
 x=app=scene=None
 try:
  x=subprocess.Popen([str(b.root/'root/usr/bin/Xvfb'),':98','-screen','0',f'{w}x{h}x24','-nolisten','unix','-nolisten','local','-listen','tcp','-ac'],stdout=open(out/f'x-{h}.log','w'),stderr=subprocess.STDOUT);time.sleep(2)
  app=subprocess.Popen([str(next((b.root/'apps').rglob('zapture'))),'--mcp'],env=b.env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=open(out/f'zapture-{h}.log','w'),text=True)
  control=b.MCP(app);control.call('initialize',{'protocolVersion':'2025-06-18','capabilities':{},'clientInfo':{'name':'matrix-benchmark','version':'1'}});time.sleep(2)
  for workload in ['document','animation','busy']:
   print('WORKLOAD',h,workload,flush=True)
   scene=subprocess.Popen(['python3','-c',scene_source,str(w),str(h),workload],env=b.env,stdout=open(out/f'scene-{h}-{workload}.log','w'),stderr=subprocess.STDOUT);time.sleep(2)
   jobs=[(kind,name,trial) for kind,names in [('screenshot',['Zapture','FFmpeg','ImageMagick','scrot']),('recording',['Zapture','FFmpeg'])] for name in names for trial in range(1,4)]
   rng.shuffle(jobs)
   for kind,name,trial in jobs:
    row={'test':kind,'app':name,'height':h,'width':w,'workload':workload,'trial':trial};p=None;peak=None
    try:
     path=out/f'{kind}-{name}-{h}-{workload}-{trial}.{ "png" if kind=="screenshot" else "mp4"}'
     if kind=='screenshot':
      if name=='Zapture':
       peak=Peak(app);start=time.monotonic();r=tool('zapture_screenshot',{'inline':False});row['elapsed_seconds']=time.monotonic()-start;path=pathlib.Path(r['path'])
      else:
       command={'FFmpeg':['ffmpeg','-v','error','-y','-f','x11grab','-i',b.env['DISPLAY'],'-frames:v','1',str(path)],'ImageMagick':['import','-window','root',str(path)],'scrot':[str(b.root/'root/usr/bin/scrot'),str(path)]}[name]
       start=time.monotonic();p=subprocess.Popen(command,env=b.env,stdout=subprocess.DEVNULL,stderr=open(out/f'{name}-errors.log','a'));peak=Peak(p);p.wait(timeout=30);row['elapsed_seconds']=time.monotonic()-start;assert p.returncode==0
      row['sampled_peak_rss_mib']=peak.end();peak=None
      with Image.open(path) as im:
       im.load();assert im.size==(w,h);assert max(ImageStat.Stat(im.convert('RGB')).stddev)>10
      row['output_bytes']=path.stat().st_size
     else:
      if name=='Zapture':tool('zapture_start_recording',{'audio':'off'});p=app
      else:
       command=['ffmpeg','-hide_banner','-loglevel','error','-y','-thread_queue_size','64','-f','x11grab','-framerate','30','-i',b.env['DISPLAY'],'-map','0:v:0','-vf','pad=ceil(iw/2)*2:ceil(ih/2)*2','-pix_fmt','yuv420p','-c:v','libx264','-preset','ultrafast','-crf','23','-movflags','+faststart',str(path)]
       p=subprocess.Popen(command,env=b.env,stdin=subprocess.PIPE,stdout=subprocess.DEVNULL,stderr=open(out/'ffmpeg-errors.log','a'))
      time.sleep(2);row['metrics']=b.sample(p,b.a.seconds)
      if name=='Zapture':path,row['finalize_seconds']=finish()
      else:
       t=time.monotonic();p.stdin.write(b'q\n');p.stdin.flush();p.wait(timeout=30);assert p.returncode==0;row['finalize_seconds']=time.monotonic()-t
      row['video']=probe(path,w,h);row['output_bytes']=pathlib.Path(path).stat().st_size
     row['status']='ok'
    except Exception as e:
     row['status']='failed';row['error']=str(e)
     if name=='Zapture' and kind=='recording':
      try:finish()
      except Exception:pass
    finally:
     if peak:peak.end()
     if p is not app:b.stop(p)
    rows.append(row);save();print(len(rows),kind,name,h,workload,trial,row['status'],flush=True)
   b.stop(scene);scene=None
 finally:b.stop(scene);b.stop(app);b.stop(x)
print('COMPLETE',len(rows),flush=True)
