'use strict';
const $ = id => document.getElementById(id);
let deviceToken = '', me = null, file = null, recipients = [], ws = null, uploading = false;
const MAX_WEB_SIZE = 64 * 1024 * 1024, CHUNK = 8 * 1024 * 1024;
function setStatus(id, value, type='') { const el=$(id); el.textContent=value; el.className='status '+type; }
function size(n) { return n >= 1048576 ? (n/1048576).toFixed(1)+' МБ' : Math.ceil(n/1024)+' КБ'; }
function text(el, content) { el.textContent = content; return el; }
function el(tag, className, content='') { const n=document.createElement(tag); if(className)n.className=className; n.textContent=content;return n; }
async function api(path, {admin=false, method='GET', json=null, body=null}={}) {
 const token=admin?$('adminToken').value.trim():deviceToken;
 if(!token) throw new Error('Спочатку введи токен.');
 const headers={Authorization:'Bearer '+token};
 if(json!==null) headers['Content-Type']='application/json';
 const res=await fetch(path,{method,headers,body:json!==null?JSON.stringify(json):body,cache:'no-store'});
 if(!res.ok) {
   let info; try { info=await res.json(); } catch { info={}; }
   throw new Error((info.error||'HTTP '+res.status));
 }
 return res.status===204?null:await res.json();
}
async function connect() {
 deviceToken=$('deviceToken').value.trim();
 if(!deviceToken) return;
 try {
  me=await api('/api/v1/me');
  $('identity').textContent='Поточний пристрій: '+me.name;
  $('connection').textContent='● '+me.name;
  $('connection').style.color='#35df9e';
  setStatus('loginMessage','Авторизація успішна.','success');
  await Promise.all([loadDevices(),loadInbox()]);
  live();
 } catch(e) {
  me=null;setStatus('loginMessage',e.message,'error');$('connection').textContent='● Не підключено';
 }
}
function disconnect() {
 deviceToken='';me=null;file=null;$('deviceToken').value='';$('recipient').replaceChildren(new Option('Не підключено',''));
 $('deviceList').textContent='Підключення відсутнє.';$('inbox').textContent='Підключення відсутнє.';
 $('connection').textContent='● Не підключено';$('send').disabled=true;
 if(ws){ws.close();ws=null}setStatus('loginMessage','Сеанс завершено.');
}
async function loadDevices() {
 if(!me)return;
 const list=await api('/api/v1/devices');recipients=list.filter(d=>d.id!==me.id);
 $('recipient').replaceChildren(new Option('Оберіть пристрій',''),...recipients.map(d=>new Option(d.name,d.id)));
 const container=$('deviceList');container.replaceChildren();
 for(const d of list) {
  const card=el('div','file');
  const info=el('div','info');info.append(el('strong','',d.name+(d.id===me.id?' · Цей пристрій':'')),el('small','',d.id));
  card.append(el('span','icon','▣'),info);container.append(card);
 }
}
function selectFile(candidate) {
 file=candidate||null;$('send').disabled=!file || !me || uploading;
 if(!file){setStatus('selectedFile','');return}
 setStatus('selectedFile',file.name+' · '+size(file.size)+(file.size>MAX_WEB_SIZE?' · Занадто великий для вебклієнта':''));
 if(file.size>MAX_WEB_SIZE||file.size===0){$('send').disabled=true;setStatus('uploadMessage','Вебклієнт підтримує файли 1 байт – 64 МБ. Для великих файлів потрібен нативний клієнт.','error');}
}
async function sha256(blob) {
 const data=await blob.arrayBuffer();
 const digest=await crypto.subtle.digest('SHA-256',data);
 return [...new Uint8Array(digest)].map(v=>v.toString(16).padStart(2,'0')).join('');
}
async function send() {
 if(!me || !file || uploading || !($('recipient').value) || file.size>MAX_WEB_SIZE)return;
 uploading=true;$('send').disabled=true;
 const progress=$('uploadProgress');progress.classList.remove('hidden');progress.value=0;
 try {
  setStatus('uploadMessage','Розрахунок SHA-256…');
  const hash=await sha256(file);
  const created=await api('/api/v1/transfers',{method:'POST',json:{
    to_device_id:$('recipient').value,filename:file.name,
    size_bytes:file.size,sha256:hash
  }});
  const id=created.transfer.id;
  for(let i=0;i<created.chunk_count;i++){
   const slice=file.slice(i*CHUNK,Math.min(file.size,(i+1)*CHUNK));
   const res=await fetch('/api/v1/transfers/'+encodeURIComponent(id)+'/chunks/'+i,{
    method:'PUT',headers:{Authorization:'Bearer '+deviceToken},
    body:slice,cache:'no-store'
   });
   if(!res.ok) {const j=await res.json().catch(()=>({}));throw new Error(j.error||'Не вдалося надіслати блок '+i);}
   progress.value=Math.round(100*(i+1)/created.chunk_count);
   setStatus('uploadMessage','Передавання… '+progress.value+'%');
  }
  await api('/api/v1/transfers/'+encodeURIComponent(id)+'/finish',{method:'POST'});
  setStatus('uploadMessage','✓ Файл на сервері. Одержувач побачить його у вхідних.','success');
  file=null;$('uploadInput').value='';setStatus('selectedFile','');
 }catch(e){setStatus('uploadMessage',e.message,'error')}
 finally{uploading=false;$('send').disabled=!file; }
}
async function loadInbox() {
 if(!me)return;
 try{
  const list=await api('/api/v1/inbox');const root=$('inbox');root.replaceChildren();
  if(!list.length)root.append(el('p','subtle','Нових файлів поки немає.'));
  for(const item of list) {
   const row=el('div','file'),info=el('div','info');
   info.append(el('strong','',item.filename),el('small','',size(item.size_bytes)+' · '+new Date(item.created_at*1000).toLocaleString()));
   const download=el('button','','⇩ Отримати');
   download.addEventListener('click',()=>downloadFile(item));
   const ack=el('button','','✓ Доставлено');
   ack.addEventListener('click',()=>acknowledge(item));
   row.append(el('span','icon',item.filename.toLowerCase().endsWith('.apk')?'◆':'▤'),info,download,ack);
   root.append(row);
  }
  setStatus('inboxMessage','');
 }catch(e){setStatus('inboxMessage',e.message,'error')}
}
async function downloadFile(item) {
 try {
  setStatus('inboxMessage','Отримуємо й перевіряємо '+item.filename+'…');
  if(item.size_bytes>MAX_WEB_SIZE)throw new Error('Вебклієнт завантажує до 64 МБ. Для більшого файлу потрібен нативний клієнт.');
  const response=await fetch('/api/v1/transfers/'+encodeURIComponent(item.id)+'/download',{
   headers:{Authorization:'Bearer '+deviceToken},cache:'no-store'
  });
  if(!response.ok)throw new Error('HTTP '+response.status);
  const bytes=await response.arrayBuffer();
  const hash=await crypto.subtle.digest('SHA-256',bytes);
  const actual=[...new Uint8Array(hash)].map(v=>v.toString(16).padStart(2,'0')).join('');
  if(actual!==item.sha256)throw new Error('Контрольна сума не збігається! Не відкривай файл.');
  const obj=URL.createObjectURL(new Blob([bytes],{type:'application/octet-stream'}));
  const a=document.createElement('a');a.href=obj;a.download=item.filename;a.click();
  setTimeout(()=>URL.revokeObjectURL(obj),30000);
  setStatus('inboxMessage','✓ SHA-256 перевірено. Після збереження натисни «Доставлено».','success');
 }catch(e){setStatus('inboxMessage',e.message,'error')}
}
async function acknowledge(item) {
 if(!confirm('Підтвердити, що файл збережено на пристрої? Після цього його буде видалено із серверного сховища.'))return;
 try {
  await api('/api/v1/transfers/'+encodeURIComponent(item.id)+'/ack',{method:'POST',json:{sha256:item.sha256}});
  await loadInbox();
 }catch(e){setStatus('inboxMessage',e.message,'error')}
}
async function register() {
 const name=$('newDevice').value.trim();
 if(!name)return setStatus('registerMessage','Введи назву нового пристрою.','error');
 try{
  const r=await api('/api/v1/admin/devices',{admin:true,method:'POST',json:{name}});
  setStatus('registerMessage','Створено '+r.name+'. Скопіюй токен зараз.','success');
  $('createdToken').textContent=r.token;
  $('createdToken').classList.remove('hidden');
  $('newDevice').value='';
  if(me)await loadDevices();
 }catch(e){setStatus('registerMessage',e.message,'error')}
}
async function changePublicDomain(){
 const input=$('adminOrigin').value.trim();
 if(!input) return setStatus('adminDomainMessage','Введи HTTPS-адресу.', 'error');
 try{
  const changed=await api('/api/v1/admin/origin',{admin:true,method:'POST',json:{public_url:input}});
  setStatus('adminDomainMessage','✓ Нову адресу збережено: '+changed.public_url+
    '. Тепер зміни DNS, SSL і server_name у aaPanel/Cloudflare.', 'success');
 }catch(e){
  setStatus('adminDomainMessage',e.message,'error');
 }
}
function live(){
 if(ws){ws.close();ws=null}
 const url=(location.protocol==='https:'?'wss://':'ws://')+location.host+'/ws';
 try {
  const stream=new WebSocket(url);ws=stream;
  stream.addEventListener('open',()=>stream.send(JSON.stringify({type:'auth',token:deviceToken})));
  stream.addEventListener('message',event=>{
   try {const msg=JSON.parse(event.data);if(msg.type==='transfer_ready'&&msg.device_id===me?.id)loadInbox()}catch{}
  });
 }catch{}
}
$('connect').addEventListener('click',connect);$('disconnect').addEventListener('click',disconnect);
$('refresh').addEventListener('click',loadInbox);$('register').addEventListener('click',register);
$('changeDomain').addEventListener('click',changePublicDomain);
$('adminOrigin').value=location.protocol==='https:'?location.origin:'';
$('choose').addEventListener('click',()=>$('uploadInput').click());
$('uploadInput').addEventListener('change',e=>selectFile(e.target.files?.[0]));
$('send').addEventListener('click',send);
const drop=$('drop');
drop.addEventListener('dragover',e=>{e.preventDefault();drop.classList.add('drag')});
drop.addEventListener('dragleave',()=>drop.classList.remove('drag'));
drop.addEventListener('drop',e=>{e.preventDefault();drop.classList.remove('drag');selectFile(e.dataTransfer.files?.[0])});
setInterval(()=>{if(me&&document.visibilityState==='visible')loadInbox().catch(()=>{})},15000);
