'use strict';
const $ = id => document.getElementById(id);
let deviceToken = '', me = null, file = null, recipients = [], ws = null, uploading = false;
let pendingUpload = null;
// Download completion is not proof of saving; still require an explicit owner confirmation.
const verifiedDownloads = new Set();
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
 verifiedDownloads.clear();
 pendingUpload=null;$('e2eeKey').value='';$('e2eeKeyBox').classList.add('hidden');
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
 if(file!==candidate){pendingUpload=null;$('e2eeKey').value='';$('e2eeKeyBox').classList.add('hidden');}
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

const hex = bytes => [...bytes].map(v=>v.toString(16).padStart(2,'0')).join('');
function keyFromHex(value) {
 if(!/^[0-9a-fA-F]{64}$/.test(value))throw new Error('Ключ має містити 64 шістнадцяткові символи.');
 return Uint8Array.from(value.match(/.{2}/g), part=>parseInt(part,16));
}
async function seal(source) {
 if(!isSecureContext || !crypto.subtle)throw new Error('E2EE працює лише через HTTPS.');
 const material=crypto.getRandomValues(new Uint8Array(32));
 const iv=crypto.getRandomValues(new Uint8Array(12));
 const key=await crypto.subtle.importKey('raw',material,{name:'AES-GCM'},false,['encrypt']);
 const ciphertext=await crypto.subtle.encrypt({name:'AES-GCM',iv},key,await source.arrayBuffer());
 return {payload:new Blob([new TextEncoder().encode('SYNCHRO1'),iv,ciphertext]),key:hex(material)};
}
async function unseal(data,secret) {
 const bytes=new Uint8Array(data);
 if(bytes.length<37 || new TextDecoder().decode(bytes.subarray(0,8))!=='SYNCHRO1')
   throw new Error('Невідомий формат E2EE.');
 const key=await crypto.subtle.importKey('raw',keyFromHex(secret),{name:'AES-GCM'},false,['decrypt']);
 try {
  return await crypto.subtle.decrypt({name:'AES-GCM',iv:bytes.subarray(8,20)},key,bytes.subarray(20));
 } catch { throw new Error('Неправильний E2EE-ключ або пошкоджений файл.'); }
}
async function awaitFinalization(id) {
 for(let i=0;i<30;i++){
  const state=await api('/api/v1/transfers/'+encodeURIComponent(id));
  if(state.status==='ready')return;
  if(state.status!=='finalizing')throw new Error('Фіналізація не вдалася. Повтори передавання.');
  setStatus('uploadMessage','Перевірка й збирання на сервері…');
  await new Promise(resolve=>setTimeout(resolve,2000));
 }
 throw new Error('Збирання ще триває. Повторно натисни «Надіслати», щоб перевірити статус.');
}
async function send() {
 if(!me || !file || uploading || !$('recipient').value || file.size>MAX_WEB_SIZE || file.size===0)return;
 uploading=true;$('send').disabled=true;
 const recipient=$('recipient').value, encrypted=$('encrypt').checked;
 const progress=$('uploadProgress');progress.classList.remove('hidden');progress.value=0;
 try {
  if(!pendingUpload || pendingUpload.source!==file ||
     pendingUpload.recipient!==recipient || pendingUpload.encrypted!==encrypted) {
   setStatus('uploadMessage',encrypted?'Шифруємо AES-256-GCM у браузері…':'Розрахунок SHA-256…');
   const sealed=encrypted?await seal(file):{payload:file,key:null};
   pendingUpload={source:file,recipient,encrypted,payload:sealed.payload,
    key:sealed.key,hash:await sha256(sealed.payload),id:null};
  }
  const task=pendingUpload;
  $('e2eeKeyBox').classList.toggle('hidden',!task.key);
  $('e2eeKey').value=task.key||'';
  if(!task.id) {
   const created=await api('/api/v1/transfers',{method:'POST',json:{
    to_device_id:recipient,filename:file.name,size_bytes:task.payload.size,
    sha256:task.hash,encrypted
   }});
   task.id=created.transfer.id;
  }
  const path='/api/v1/transfers/'+encodeURIComponent(task.id);
  const checkpoint=await api(path+'/chunks');
  if(checkpoint.status==='uploading') {
   const already=new Set(checkpoint.uploaded_indices);
   const chunkHashes=new Map((checkpoint.uploaded_chunks||[]).map(item=>[item.index,item.sha256]));
   if(already.size && !chunkHashes.size)throw new Error('Сервер потребує оновлення до Synchro v0.5.1.');
   for(let i=0;i<checkpoint.chunk_count;i++){
    const slice=task.payload.slice(i*CHUNK,Math.min(task.payload.size,(i+1)*CHUNK));
    let valid=false;
    if(already.has(i) && chunkHashes.has(i)){
     const localHash=await sha256(slice);
     valid=localHash===chunkHashes.get(i);
     if(!valid){
      const gone=await fetch(path+'/chunks/'+i,{
       method:'DELETE',headers:{Authorization:'Bearer '+deviceToken},cache:'no-store'
      });
      if(!gone.ok)throw new Error('Не вдалося видалити пошкоджений чанк '+i);
     }
    }
    if(!valid){
     const response=await fetch(path+'/chunks/'+i,{
      method:'PUT',headers:{Authorization:'Bearer '+deviceToken},body:slice,cache:'no-store'
     });
     if(!response.ok){const j=await response.json().catch(()=>({}));throw new Error(j.error||'Блок '+i+' не прийнято');}
    }
    progress.value=Math.round(100*(i+1)/checkpoint.chunk_count);
    setStatus('uploadMessage','Передавання… '+progress.value+'%');
   }
   await api(path+'/finish',{method:'POST'});
  }
  if(checkpoint.status!=='ready') await awaitFinalization(task.id);
  setStatus('uploadMessage','✓ Файл готовий для одержувача.','success');
  pendingUpload=null;file=null;$('uploadInput').value='';setStatus('selectedFile','');
 }catch(e){setStatus('uploadMessage',e.message+' Повторна спроба продовжить із готових блоків.','error');}
 finally{uploading=false;$('send').disabled=!file || !me;}
}

async function loadInbox() {
 if(!me)return;
 try{
  const list=await api('/api/v1/inbox');const root=$('inbox');root.replaceChildren();
  if(!list.length)root.append(el('p','subtle','Нових файлів поки немає.'));
  for(const item of list) {
   const row=el('div','file'),info=el('div','info');
   info.append(el('strong','',(item.encrypted?'🔐 ':'')+item.filename),el('small','',size(item.size_bytes)+' · '+new Date(item.created_at*1000).toLocaleString()));
   const download=el('button','','⇩ Отримати');
   download.addEventListener('click',()=>downloadFile(item));
   const ack=el('button','',item.status==='delivered'?'✓ Підтверджено':'✓ Доставлено');
   ack.disabled=item.status==='delivered'||!verifiedDownloads.has(item.id);
   if(!ack.disabled) ack.title='SHA-256 перевірено. Підтверди, що файл збережено.';
   else if(item.status!=='delivered') ack.title='Спочатку завантаж і перевір файл.';
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
  if(item.size_bytes>MAX_WEB_SIZE+64)throw new Error('Вебклієнт завантажує до 64 МБ.');
  const res=await fetch('/api/v1/transfers/'+encodeURIComponent(item.id)+'/download',{
   headers:{Authorization:'Bearer '+deviceToken},cache:'no-store'
  });
  if(!res.ok)throw new Error('HTTP '+res.status);
  const bytes=await res.arrayBuffer();
  if(hex(new Uint8Array(await crypto.subtle.digest('SHA-256',bytes)))!==item.sha256)
   throw new Error('SHA-256 не збігається. Не відкривай файл!');
  let result=bytes;
  if(item.encrypted){
   const secret=prompt('Введи E2EE-ключ, отриманий ОКРЕМО від відправника (64 символи)');
   if(!secret)throw new Error('Для E2EE потрібен секретний ключ.');
   result=await unseal(bytes,secret.trim());
  }
  verifiedDownloads.add(item.id);
  const url=URL.createObjectURL(new Blob([result],{type:'application/octet-stream'}));
  const link=document.createElement('a');link.href=url;link.download=item.filename;link.click();
  setTimeout(()=>URL.revokeObjectURL(url),30000);
  setStatus('inboxMessage','✓ SHA-256'+(item.encrypted?' і AES-GCM':'')+' перевірено. Підтвердь збереження файлу.','success');
  await loadInbox();
 }catch(e){setStatus('inboxMessage',e.message,'error');}
}
async function acknowledge(item) {
 if(!verifiedDownloads.has(item.id)) return setStatus('inboxMessage','Спочатку завантаж файл і перевір SHA-256.','error');
 if(!confirm('Підтвердити, що файл збережено на пристрої? Сервер зберігатиме резервну копію до години для відновлення.'))return;
 try {
  await api('/api/v1/transfers/'+encodeURIComponent(item.id)+'/ack',{method:'POST',json:{sha256:item.sha256}});
  verifiedDownloads.delete(item.id);
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
async function refreshServerStats(){
 try{
  const s=await api('/api/v1/admin/stats',{admin:true});
  const gib=n=>(n/1073741824).toFixed(2)+' ГіБ';
  $('serverStats').textContent=
    'Пристроїв: '+s.devices+' · Завантажуються: '+s.uploading+
    ' · У вхідних: '+s.ready+' · Доставлено: '+s.delivered+
    ' · Зарезервовано: '+gib(s.reserved_bytes)+' / '+gib(s.quota_bytes)+
    ' · Вільно на VPS: '+gib(s.free_bytes)+
    ' · Термін зберігання: '+s.ttl_hours+' год';
 }catch(e){
  $('serverStats').textContent='Не вдалося отримати статистику: '+e.message;
 }
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
$('refreshServerStats').addEventListener('click',refreshServerStats);
$('adminOrigin').value=location.protocol==='https:'?location.origin:'';
$('choose').addEventListener('click',()=>$('uploadInput').click());
$('uploadInput').addEventListener('change',e=>selectFile(e.target.files?.[0]));
$('send').addEventListener('click',send);
$('encrypt').addEventListener('change',()=>{pendingUpload=null;$('e2eeKey').value='';$('e2eeKeyBox').classList.add('hidden');});
$('copyE2EEKey').addEventListener('click',()=>navigator.clipboard.writeText($('e2eeKey').value).catch(()=>{}));
const drop=$('drop');
drop.addEventListener('dragover',e=>{e.preventDefault();drop.classList.add('drag')});
drop.addEventListener('dragleave',()=>drop.classList.remove('drag'));
drop.addEventListener('drop',e=>{e.preventDefault();drop.classList.remove('drag');selectFile(e.dataTransfer.files?.[0])});
setInterval(()=>{if(me&&document.visibilityState==='visible')loadInbox().catch(()=>{})},15000);
