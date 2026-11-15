"""Build charts and a report from linux_extended.py JSON results."""
import argparse,json,pathlib,statistics
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
p=argparse.ArgumentParser();p.add_argument('input',type=pathlib.Path);p.add_argument('output',type=pathlib.Path);a=p.parse_args();a.output.mkdir(parents=True,exist_ok=True)
rows=json.loads(a.input.read_text());ok=[r for r in rows if r['status']=='ok']
heights=[720,1080,1440];colors=['#2563eb','#0891b2','#e85c42']
plt.rcParams.update({'font.family':'DejaVu Sans','font.size':11,'axes.spines.top':False,'axes.spines.right':False,'svg.fonttype':'none'})
fig,axes=plt.subplots(1,3,figsize=(14,4.6))
for ax,key,title,unit in [(axes[0],'cpu_core_pct_mean','Recording CPU','% of one CPU core'),(axes[1],'rss_mib_median','Recording memory','MiB, including FFmpeg')]:
 groups=[[r['metrics'][key] for r in ok if r['test']=='resolution' and r['height']==h] for h in heights]
 vals=[statistics.median(g) if g else float('nan') for g in groups]
 ax.bar([str(h)+'p' for h in heights],vals,color=colors)
 for i,g in enumerate(groups):
  ax.scatter([i]*len(g),g,c='black',s=20,zorder=3)
  if g:ax.text(i,max(g)*1.04,f'{vals[i]:.1f}',ha='center')
 ax.set_title(title);ax.set_ylabel(unit);ax.set_ylim(bottom=0,top=max(vals)*1.2);ax.grid(axis='y',alpha=.15)
groups=[[r['seconds']*1000 for r in ok if r['test']=='screenshot' and r['height']==h] for h in heights]
axes[2].boxplot(groups,tick_labels=[str(h)+'p' for h in heights],showmeans=True)
axes[2].set_title('Screenshot response time');axes[2].set_ylabel('Milliseconds, 5 captures per resolution');axes[2].set_ylim(bottom=0)
fig.suptitle('Zapture Linux cloud tests • software rendering • 30 fps',fontsize=16,y=1.02)
fig.text(.5,-.02,'Recording: two 15-second measurement windows per resolution. Black dots show individual trials. Synthetic scene; no audio.',ha='center',fontsize=10)
fig.tight_layout();fig.savefig(a.output/'resolution-tests.svg',bbox_inches='tight');fig.savefig(a.output/'resolution-tests.png',dpi=160,bbox_inches='tight');plt.close(fig)
long=next((r for r in ok if r['test']=='sustained'),None)
if long:
 samples=long['metrics']['samples'];fig,axes=plt.subplots(2,1,figsize=(11,6),sharex=True)
 for ax,key,label,col in [(axes[0],'rss_mib','Recorder memory (MiB)','#2563eb'),(axes[1],'cpu_core_pct','CPU (% of one core)','#e85c42')]:
  ax.plot([x['elapsed'] for x in samples],[x[key] for x in samples],color=col,lw=1.4);ax.set_ylabel(label);ax.grid(alpha=.15);ax.set_ylim(bottom=0)
 axes[1].set_xlabel('Seconds after warmup');axes[0].set_title('Two-minute recording at 720p/30 fps — full recorder process tree')
 fig.tight_layout();fig.savefig(a.output/'sustained-test.svg');fig.savefig(a.output/'sustained-test.png',dpi=160);plt.close(fig)
lines=['# Extended Linux tests — 15 November 2026','','These are functional and performance tests of the released Linux alpha, using a synthetic X11 desktop in a cloud container. They are not Windows results or a competitive ranking.','','## What was tested','','| Test | Passed | Total |','|---|---:|---:|']
for name in ['screenshot','resolution','repeat-cycle','sustained']:
 selected=[r for r in rows if r['test']==name];lines.append(f'| {name} | {sum(r["status"]=="ok" for r in selected)} | {len(selected)} |')
fail=[r for r in rows if r['status']!='ok'];lines+=['',f'Suite errors: {len(fail)}. All errors, if any, are retained in the raw JSON.','','## Resolution and screenshot tests','','![Resolution tests](resolution-tests.svg)','','| Resolution | Recording CPU (one core) | Recording RAM (RSS) | Screenshot median |','|---|---:|---:|---:|']
for h in heights:
 rec=[r for r in ok if r['test']=='resolution' and r['height']==h];shot=[r for r in ok if r['test']=='screenshot' and r['height']==h]
 if rec and shot:lines.append(f'| {h}p | {statistics.median(r["metrics"]["cpu_core_pct_mean"] for r in rec):.1f}% | {statistics.median(r["metrics"]["rss_mib_median"] for r in rec):.1f} MiB | {statistics.median(r["seconds"] for r in shot)*1000:.0f} ms |')
if long:
 samples=long['metrics']['samples'];first=statistics.median(x['rss_mib'] for x in samples[:20]);last=statistics.median(x['rss_mib'] for x in samples[-20:]);lines+=['','## Sustained recording','','![Sustained recording](sustained-test.svg)','',f'The two-minute 720p recording passed decoding and changing-content validation. Median RSS in the first/last ~10 seconds: {first:.1f}/{last:.1f} MiB (difference {last-first:+.1f} MiB). This short observation cannot rule out a memory leak over hours.']
lines+=['','## Method and limits','','- Same published 0.1.0-alpha.2 binary and environment as the [earlier benchmark](../linux-cloud-2026-11-15/README.md).','- Screenshots: 5 sequential calls per resolution; timings cover MCP request through saved PNG response, with inline image transfer disabled. Each PNG was fully decoded and its dimensions checked.','- Resolution tests: 2 recordings per resolution; 2-second warmup plus 15-second resource sampling. Full recorder process tree includes FFmpeg. CPU 100% equals one logical core; summed RSS can double-count shared pages.','- Repeat cycles: 5 consecutive 3-second recordings in the same app process, with decoded video and an additional idle stop after each cycle.','- Video validation: ffprobe frame count and dimensions; two decoded sample frames must differ. This detects obvious blank/static output, not every dropped frame or quality defect.','- Sustained test: 120-second measurement after warmup, followed by stop/finalization and validation.','- Resolution order was 720p, 1080p, then 1440p, with no randomized scheduling. Two recording trials per resolution are exploratory, not statistically conclusive.','- The synthetic source expands its desktop and motion range, but element sizes are not all scaled proportionally. Screen content, software rendering and cloud scheduling affect measurements.','- All recording tests use 30 fps, x264 ultrafast CRF 23 and audio off. No quality/fps setting changes, hardware audio tests, GPU tests or desktop OS compatibility claims are made.','- OBS remains excluded because its prior capture produced black frames.','','## Run it','','Use the [existing package setup](../../../scripts/benchmarks/README.md), then:','','```sh','python3 scripts/benchmarks/linux_extended.py --root "$BENCH"','python3 scripts/benchmarks/plot_extended.py "$BENCH/extended-results/results.json" docs/benchmarks/linux-extended-2026-11-15','```','','[Raw results](results.json). No production app code was changed.']
(a.output/'README.md').write_text('\n'.join(lines)+'\n');(a.output/'results.json').write_text(json.dumps(rows,separators=(',',':'))+'\n')
