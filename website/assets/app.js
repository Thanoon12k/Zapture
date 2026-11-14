'use strict';
const $=id=>document.getElementById(id);let running=false,paused=false,elapsed=0,toastTimer;
const status=$('demo-state'),timer=$('demo-time');
function notify(message){$('demo-toast').textContent=message;$('demo-toast').classList.add('show');clearTimeout(toastTimer);toastTimer=setTimeout(()=>$('demo-toast').classList.remove('show'),3200)}
function render(){timer.textContent=String(Math.floor(elapsed/60)).padStart(2,'0')+':'+String(elapsed%60).padStart(2,'0');$('record').disabled=running;$('pause').disabled=!running;$('stop').disabled=!running;$('pause').textContent=paused?'▶':'II';$('pause').setAttribute('aria-label',paused?'Resume recording demo':'Pause recording demo');status.textContent=running?(paused?'Paused. Take your time.':'Recording preview. Your ideas, in motion.'):'Ready when you are.'}
$('record').addEventListener('click',()=>{running=true;paused=false;elapsed=0;render()});$('pause').addEventListener('click',()=>{paused=!paused;render()});$('stop').addEventListener('click',()=>{running=false;paused=false;render();notify('✓ Recording preview complete. That was easy.')});$('shot').addEventListener('click',()=>notify('✓ Screenshot preview complete. Nice shot!'));setInterval(()=>{if(running&&!paused){elapsed++;render()}},1000);
$('copy-config').addEventListener('click',async()=>{try{await navigator.clipboard.writeText($('mcp-code').textContent);$('copy-config').textContent='Copied!';setTimeout(()=>$('copy-config').textContent='Copy config',2200)}catch{const range=document.createRange();range.selectNodeContents($('mcp-code'));const selection=window.getSelection();selection.removeAllRanges();selection.addRange(range);$('copy-config').textContent='Select & copy';}});
const art=document.querySelector('.hero-art'),scene=document.querySelector('.glass-scene');if(matchMedia('(hover:hover) and (prefers-reduced-motion:no-preference)').matches){art.addEventListener('pointermove',e=>{const r=art.getBoundingClientRect(),x=(e.clientX-r.left)/r.width-.5,y=(e.clientY-r.top)/r.height-.5;scene.style.transform=`rotateY(${-12+x*10}deg) rotateX(${8-y*8}deg) rotateZ(-5deg)`});art.addEventListener('pointerleave',()=>scene.style.transform='')}

// Anonymous daily browser count. No IP, fingerprint or visitor identifier stored.
fetch('/api/visit',{method:'POST',headers:{'X-Zapture-Visit':'1'},credentials:'same-origin',cache:'no-store'})
.then(r=>{if(!r.ok)throw new Error('Counter unavailable');return r.json()})
.then(data=>{if(Number.isSafeInteger(data.visits))$('visit-count').textContent=data.visits.toLocaleString()})
.catch(()=>{$('visit-count').parentElement.textContent='Visit counter temporarily unavailable'});
