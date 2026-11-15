"""Generate 54 comparison charts and report from validated matrix results."""
import json,pathlib,statistics,argparse,hashlib
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
p=argparse.ArgumentParser();p.add_argument('--root',type=pathlib.Path,required=True);p.add_argument('--repo',type=pathlib.Path,required=True);a=p.parse_args()
rows=json.loads((a.root/'matrix-results/results.json').read_text());ok=[x for x in rows if x['status']=='ok']
from collections import Counter
counts=Counter((x['test'],x['app'],x['height'],x['workload']) for x in rows)
assert len(rows)==162 and len(counts)==54 and set(counts.values())=={3}, 'Incomplete benchmark matrix; refusing to publish'
assert len({(x['test'],x['app'],x['height'],x['workload'],x['trial']) for x in rows})==162
dest=a.repo/'docs/benchmarks/linux-matrix-2026-11-15';dest.mkdir(parents=True,exist_ok=True);assets=a.repo/'website/assets'
metrics=[('record-cpu','Recording CPU','% of one logical core','recording',lambda r:r['metrics']['cpu_core_pct_mean']),('record-ram','Recording memory','MiB RSS, including child processes','recording',lambda r:r['metrics']['rss_mib_median']),('record-fps','Encoded frame rate','Frames / output duration (not unique frames)','recording',lambda r:r['video']['delivered_fps']),('shot-time','Screenshot response time','Milliseconds to saved PNG response','screenshot',lambda r:r['elapsed_seconds']*1000),('shot-ram','Screenshot sampled peak memory','MiB RSS; 10 ms samples, not guaranteed peak','screenshot',lambda r:r['sampled_peak_rss_mib']),('shot-size','Screenshot file size','KiB PNG; default compression settings','screenshot',lambda r:r['output_bytes']/1024)]
plt.rcParams.update({'font.family':'DejaVu Sans','svg.fonttype':'none','font.size':11,'axes.spines.top':False,'axes.spines.right':False,'svg.hashsalt':'zapture-matrix'})
colors={'Zapture':'#e85c42','FFmpeg':'#527b9a','ImageMagick':'#69977b','scrot':'#a48d5b'}
summary=[];charts=[]
for h in [720,1080,1440]:
 for scene in ['document','animation','busy']:
  for key,title,unit,kind,get in metrics:
   names=['Zapture','FFmpeg'] if kind=='recording' else ['Zapture','FFmpeg','ImageMagick','scrot']
   groups=[[get(r) for r in ok if r['height']==h and r['workload']==scene and r['app']==n and r['test']==kind and get(r) is not None] for n in names]
   vals=[statistics.median(g) if g else 0 for g in groups]
   fig,ax=plt.subplots(figsize=(7.2,4.2));ax.bar(names,vals,color=[colors[n] for n in names],width=.6)
   for i,g in enumerate(groups):
    if g:
     ax.scatter([i]*len(g),g,s=14,color='#263026',zorder=4)
     ax.text(i,max(g)+max(vals)*.04,f'{vals[i]:.1f}',ha='center',fontsize=11)
    else:ax.text(i,0,'No valid data',ha='center',fontsize=9)
   ax.set_ylim(0,max(max(vals)*1.25,.1));ax.set_title(f'{title} · {h}p / {scene}',loc='left',pad=18,fontweight='bold');ax.set_ylabel(unit);ax.grid(axis='y',alpha=.15);ax.set_axisbelow(True)
   fig.text(.5,.015,'Median of 3 runs; dots show individual trials. Linux cloud, software rendering, audio off.',ha='center',fontsize=8)
   fig.tight_layout(rect=(0,.05,1,1));filename=f'matrix-{h}-{scene}-{key}.svg';fig.savefig(assets/filename,metadata={'Date':None});plt.close(fig)
   charts.append({'height':h,'workload':scene,'metric':key,'title':title,'unit':unit,'asset':filename,'values':[{'app':n,'median':v,'samples':g} for n,v,g in zip(names,vals,groups)]})
  for kind,names in [('recording',['Zapture','FFmpeg']),('screenshot',['Zapture','FFmpeg','ImageMagick','scrot'])]:
   for name in names:
    g=[r for r in rows if r['height']==h and r['workload']==scene and r['test']==kind and r['app']==name];valid=[r for r in g if r['status']=='ok'];d={'height':h,'workload':scene,'test':kind,'app':name,'passed':len(valid),'runs':len(g)}
    for key,title,unit,mkind,get in metrics:
     v=[get(r) for r in valid if get(r) is not None] if mkind==kind else []
     if v:d[key]=statistics.median(v)
    summary.append(d)
data={'date':'2026-11-15','runs':len(rows),'passed':len(ok),'failed':len(rows)-len(ok),'scenarios':len(summary),'charts':charts,'summary':summary}
(assets/'matrix-summary.json').write_text(json.dumps(data,separators=(',',':'))+'\n');(dest/'summary.json').write_text(json.dumps(data,indent=2)+'\n');(dest/'results.json').write_text(json.dumps(rows,separators=(',',':'))+'\n')
(assets/'matrix-results.json').write_text((dest/'results.json').read_text())
for name in ['versions.json','environment.json']:(dest/name).write_bytes((a.root/'matrix-results'/name).read_bytes())
size=json.loads((a.root/'download-sizes.json').read_text());(dest/'release-assets.json').write_text(json.dumps(size,indent=2)+'\n');selected=[]
for repo in ['Thanoon12k/Zapture','ShareX/ShareX','obsproject/obs-studio','greenshot/greenshot']:
 d=next(x for x in size if x['repository']==repo)
 asset=next(x for x in d['assets'] if x['name'].endswith('.zip') and ('Windows-x64' in x['name'] or 'portable-x64' in x['name'] or 'PORTABLE' in x['name']) and 'PDB' not in x['name'])
 selected.append({'app':repo.split('/')[-1],'version':d['version'],'asset':asset['name'],'bytes':asset['size'],'mb':asset['size']/1e6,'source':d['url'],'download':asset['url']})
base=selected[0]['bytes']
for d in selected:d['zapture_smaller_pct']=(1-base/d['bytes'])*100
(dest/'download-sizes.json').write_text(json.dumps({'checked':'2026-11-15','method':'GitHub release asset size metadata; Windows portable ZIPs; packages have different features and dependencies','packages':selected},indent=2)+'\n');(assets/'matrix-downloads.json').write_bytes((dest/'download-sizes.json').read_bytes())
fig,ax=plt.subplots(figsize=(9,4));names=[d['app'] for d in selected];vals=[d['mb'] for d in selected];ax.barh(names,vals,color=['#e85c42','#527b9a','#69977b','#a48d5b']);ax.invert_yaxis();ax.set_xlim(0,max(vals)*1.25)
for i,v in enumerate(vals):ax.text(v+2,i,f'{v:.2f} MB',va='center')
ax.set_xlabel('Download size (decimal MB)');ax.set_title('Windows portable ZIP downloads · published release assets',loc='left');fig.text(.5,.01,'Checked 15 November 2026. Different features/dependencies. Not installed footprint or runtime performance.',ha='center',fontsize=8);fig.tight_layout(rect=(0,.04,1,1));fig.savefig(assets/'matrix-download-sizes.svg',metadata={'Date':None});plt.close(fig)
lines=['# Linux comparison matrix — 15 November 2026','',f'{len(summary)} distinct scenarios; {len(rows)} total runs; {len(ok)} passed; {len(rows)-len(ok)} failed. Three repetitions per scenario. 54 comparison plots plus one Windows package-size chart.','','## Scope','','Screenshots: Zapture 0.1.0-alpha.2, FFmpeg, ImageMagick and scrot. Recording: Zapture and a matched FFmpeg command-line baseline. Three resolutions (720p, 1080p, 1440p) × three synthetic workloads (document, animation, busy). See versions.json for exact comparator builds. OBS was excluded because its earlier capture was black; this report makes no OBS runtime-performance claim.','','## Measurement protocol','','- A fixed random seed shuffles app/trial order inside each resolution/workload group. Resolution and workload groups are ordered, not randomized.','- A synthetic Tk canvas covers the virtual display for all tools. It contains document lines, 8 moving rectangles, or 160 moving rectangles. It raises above app controls. Visual distributions are comparable; frames are not synchronized pixel-for-pixel.','- The Zapture UI remains running throughout each resolution group, including baseline runs, to keep the scene environment consistent. Baseline resource accounting excludes that idle UI; Zapture measurements include its UI plus FFmpeg children. Workload generator and Xvfb are excluded for both.','- Recording: 30 fps, x264 ultrafast CRF23, yuv420p, MP4, audio off. Two-second warmup then eight-second measurement. CPU 100% means one logical core. RSS is summed across the process tree and can double-count shared pages; PSS is retained in raw data.','- Screenshot latency: request/launch through saved output response. Zapture uses a running MCP-enabled UI, inline image transfer disabled. Other tools are launched per screenshot. PNG compression and capture overhead differ; this is end-to-end default behavior, not an isolated encoder comparison.','- Screenshot memory is the maximum RSS sampled every 10 ms; short-lived peaks or children can be missed. Zapture includes its resident UI; command-line tools have no resident GUI. No screenshot CPU comparison is inferred.','- Output validation: each PNG is decoded, checked for dimensions and nontrivial variation. Each video is frame-counted and one frame at two seconds is decoded and checked for variation. This does not establish perfect motion fidelity, unique-frame rate or absence of all frame drops.','- Encoded FPS = decoded frame count / file duration. This can include duplicated frames and must not be interpreted as measured capture latency or motion quality.','- The cloud has no physical GPU. This is not a Windows, hardware-acceleration, audio, battery, thermals, gaming or multi-hour benchmark. Three short repeats do not establish population-level statistical significance.','','## Size comparison','','All selected size comparisons use Windows portable ZIP release assets, not installers. Sizes are GitHub metadata, checked on the report date, in decimal MB. Feature sets and bundled/system dependencies differ. Zapture is a smaller capture-focused app; this is not feature parity and does not establish RAM/CPU savings. The Linux Zapture build uses external FFmpeg, unlike its native Windows build.','','| Package | Version | ZIP size | Zapture ZIP smaller by |','|---|---|---:|---:|']
for d in selected:lines.append(f'| [{d["app"]}]({d["source"]}) | {d["version"]} | {d["mb"]:.2f} MB | {d["zapture_smaller_pct"]:.2f}% |')
lines+=['','## Reproduce','','Prepare the [existing Linux setup](../../../scripts/benchmarks/README.md), additionally extract scrot 1.12.1-1 and libimlib2t64 1.12.4-2 (Debian trixie); provide ImageMagick 7.1.1-43 and Pillow. Then:','','```sh','python3 scripts/benchmarks/linux_matrix.py --root "$BENCH" --seconds 8','python3 scripts/benchmarks/report_matrix.py --root "$BENCH" --repo .','```','','The report generator also expects download-sizes.json in the benchmark root with the saved GitHub releases response projection (repository, version, URL and asset name/size/URL). Published selected asset metadata and the original release-assets.json projection are in this report folder; copy release-assets.json to $BENCH/download-sizes.json to regenerate the report.','','[Raw data](results.json) · [Summary](summary.json) · [Versions](versions.json) · [Size sources](download-sizes.json)','','The website exposes all plots and raw samples. No production app code was modified.']
(dest/'README.md').write_text('\n'.join(lines)+'\n')
print(json.dumps({'runs':len(rows),'passed':len(ok),'scenarios':len(summary),'charts':len(charts)}))
import zipfile
with zipfile.ZipFile(assets/'matrix-charts.zip','w',zipfile.ZIP_DEFLATED,compresslevel=9) as z:
 for file in sorted(assets.glob('matrix-*.svg')):z.write(file,'charts/'+file.name)
 for file in sorted(dest.glob('*')):z.write(file,'evidence/'+file.name)
 for name in ['linux_matrix.py','linux_cloud.py','report_matrix.py']:z.write(a.repo/'scripts/benchmarks'/name,'scripts/'+name)
print('Charts and evidence ZIP:',(assets/'matrix-charts.zip').stat().st_size,'bytes')
