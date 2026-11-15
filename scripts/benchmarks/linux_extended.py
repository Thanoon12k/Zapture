"""Extended synthetic X11 tests. Pass --root as documented for linux_cloud.py."""
import json,time,subprocess,pathlib,statistics,hashlib
import linux_cloud as b
from PIL import Image
out=b.root/'extended-results';out.mkdir(exist_ok=True)
rows=[]
def save():
 (out/'results.json').write_text(json.dumps(rows,indent=2))
def tool(name,args=None):
 r=control.call('tools/call',{'name':name,'arguments':args or {}})
 for c in r.get('content',[]):
  if c.get('type')=='text':return json.loads(c['text'])
 return r
def finish():
 t=time.monotonic();tool('zapture_stop_recording')
 for _ in range(150):
  st=tool('zapture_status')
  if st['state']=='idle':
   if not st.get('message','').startswith('Saved:'):raise RuntimeError(str(st))
   return st['recording_path'],time.monotonic()-t
  time.sleep(.1)
 raise TimeoutError('Finalization')
def video(path,width,height):
 data=json.loads(subprocess.check_output(['ffprobe','-v','error','-count_frames','-select_streams','v:0','-show_entries','stream=width,height,nb_read_frames,avg_frame_rate:format=duration,size','-of','json',path]))
 stream=data['streams'][0];assert (stream['width'],stream['height'])==(width,height)
 duration=float(data['format']['duration']);frames=[]
 for sec in [duration*.25,duration*.75]:
  raw=subprocess.check_output(['ffmpeg','-v','error','-ss',str(sec),'-i',path,'-frames:v','1','-vf','scale=160:90','-pix_fmt','rgb24','-f','rawvideo','-'])
  assert len(raw)==43200;frames.append(hashlib.sha256(raw).hexdigest())
 assert frames[0]!=frames[1],'Static frames'
 data['frame_hashes']=frames;return data
for width,height in [(1280,720),(1920,1080),(2560,1440)]:
 app=scene=x=None
 try:
  print('DISPLAY',width,height,flush=True)
  x=subprocess.Popen([str(b.root/'root/usr/bin/Xvfb'),':98','-screen','0',f'{width}x{height}x24','-nolisten','unix','-nolisten','local','-listen','tcp','-ac'],stdout=open(out/f'x-{height}.log','w'),stderr=subprocess.STDOUT);time.sleep(2)
  # Scale the same source to each resolution, keeping its relative motion consistent.
  source=b.workload.read_text().replace('1280',str(width)).replace('720',str(height)).replace('range(12)','range(30)').replace("x=(time.monotonic()-start)*180%1100",f'x=(time.monotonic()-start)*{width*.14}%{width-180}')
  scene=subprocess.Popen(['python3','-c',source],env=b.env,stdout=open(out/f'scene-{height}.log','w'),stderr=subprocess.STDOUT);time.sleep(1)
  app=subprocess.Popen([str(next((b.root/'apps').rglob('zapture'))),'--mcp'],env=b.env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=open(out/f'app-{height}.log','w'),text=True)
  control=b.MCP(app);control.call('initialize',{'protocolVersion':'2025-06-18','capabilities':{},'clientInfo':{'name':'extended-benchmark','version':'1'}});time.sleep(3)
  for trial in range(1,6):
   t=time.monotonic();r=tool('zapture_screenshot',{'inline':False});elapsed=time.monotonic()-t
   with Image.open(r['path']) as im:im.load();assert im.size==(width,height)
   rows.append({'test':'screenshot','height':height,'trial':trial,'seconds':elapsed,'bytes':pathlib.Path(r['path']).stat().st_size,'status':'ok'});save()
  for trial in range(1,3):
   tool('zapture_start_recording',{'audio':'off'});time.sleep(2);metrics=b.sample(app,15);path,stop=finish();v=video(path,width,height)
   rows.append({'test':'resolution','height':height,'trial':trial,'metrics':metrics,'stop_seconds':stop,'video':v,'status':'ok'});save();print('RECORD',height,trial,'ok',flush=True)
  if height==720:
   for trial in range(1,6):
    tool('zapture_start_recording',{'audio':'off'});time.sleep(3);path,stop=finish();v=video(path,width,height)
    # A repeated stop must leave the app idle.
    tool('zapture_stop_recording');assert tool('zapture_status')['state']=='idle'
    rows.append({'test':'repeat-cycle','height':height,'trial':trial,'stop_seconds':stop,'video':v,'status':'ok'});save()
   print('LONG RECORDING 120 seconds',flush=True)
   tool('zapture_start_recording',{'audio':'off'});time.sleep(2);metrics=b.sample(app,120);path,stop=finish();v=video(path,width,height)
   rows.append({'test':'sustained','height':height,'metrics':metrics,'stop_seconds':stop,'video':v,'status':'ok'});save()
 except Exception as e:
  rows.append({'test':'suite','height':height,'status':'failed','error':str(e)});save();print('FAILED',height,str(e),flush=True)
 finally:
  b.stop(app);b.stop(scene);b.stop(x)
print('COMPLETE',len(rows),flush=True)
