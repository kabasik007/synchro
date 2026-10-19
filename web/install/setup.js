'use strict';
const $ = id => document.getElementById(id);
let current = 1, status = null, checked = false, installing = false;
const panes = ['step1','step2','step3','step4','locked'];

function report(message, good=false) {
 const b=$('banner');
 b.textContent=message||'';
 b.classList.toggle('hidden',!message);
 b.classList.toggle('good',!!good);
}
function paint(id, value, good) {
 const e=$(id);e.textContent=value;e.classList.toggle('ok',good===true);
 e.classList.toggle('bad',good===false);
}
function show(step) {
 current=step;
 panes.forEach((id,i)=>$(id).classList.toggle('hidden',id!==('step'+step)));
 document.querySelectorAll('.step').forEach(el=>{
  const n=Number(el.dataset.step);
  el.classList.toggle('active',n===step);el.classList.toggle('complete',n<step);
 });
 if(step===4){$('step4').classList.remove('hidden');}
 window.scrollTo({top:0,behavior:'smooth'});
}
function lock() {
 panes.forEach(id=>$(id).classList.add('hidden'));
 $('locked').classList.remove('hidden');
 document.querySelectorAll('.step').forEach(el=>el.classList.remove('active'));
 window.scrollTo(0,0);
}
async function fetchJSON(url, token='', body=null) {
 const control=new AbortController();
 const timeout=setTimeout(()=>control.abort(),15000);
 try {
  const response=await fetch(url,{
   method:body?'POST':'GET',cache:'no-store',credentials:'omit',
   headers:{...(token?{Authorization:'Bearer '+token}:{}),
    ...(body?{'Content-Type':'application/json'}:{})},
   body:body?JSON.stringify(body):undefined,signal:control.signal
  });
  const json=await response.json().catch(()=>({}));
  if(!response.ok)throw new Error(json.error||'HTTP '+response.status);
  return json;
 }finally{clearTimeout(timeout)}
}
async function verifyFrontend() {
 checked=false;
 $('next1').disabled=true;
 report('');
 const secure=location.protocol==='https:';
 paint('httpsSymbol',secure?'✓':'!',secure);
 $('httpsDetails').textContent=secure?location.origin:
   'Потрібний HTTPS (сертифікат у aaPanel / Cloudflare)';
 let healthy=false;
 try{
  const x=await fetchJSON('/health');
  healthy=x.status==='ok';
  paint('apiSymbol',healthy?'✓':'!',healthy);
  $('apiDetails').textContent=healthy?'Rust API відповідає · '+(x.version||'version unknown'):
    'Сервер відповідає, але формат /health неочікуваний';
 }catch(e){
  paint('apiSymbol','!',false);
  $('apiDetails').textContent='Немає API: перевір systemd та reverse proxy ('+e.message+')';
 }
 try{
  status=await fetchJSON('/api/v1/setup/status');
  paint('dbSymbol',status.database==='SQLite'?'✓':'!',status.database==='SQLite');
  $('dbDetails').textContent=status.installed?'Сервер уже налаштований — повторний запуск заблоковано':
    'SQLite доступна · інсталяція ще не завершена';
  if(status.installed){lock();return}
 }catch(e){
  paint('dbSymbol','!',false);
  $('dbDetails').textContent='Setup API не відповідає ('+e.message+')';
 }
 checked=secure&&healthy&&status?.database==='SQLite'&&!status.installed;
 $('healthBadge').textContent=checked?'Система готова':'Потрібні дії';
 $('healthBadge').classList.toggle('ok',checked);
 $('healthBadge').classList.toggle('bad',!checked);
 $('next1').disabled=!checked;
}
async function inspect() {
 report('');
 $('next2').disabled=true;
 const token=$('adminKey').value.trim();
 if(!/^[a-fA-F0-9]{64}$/.test(token)){
  report('Введи 64-символьний секрет адміністратора, створений на VPS командою openssl rand -hex 32.');
  return;
 }
 let url;
 try{
  url=new URL($('domain').value.trim());
  if(url.protocol!=='https:' || url.pathname!=='/' || url.search || url.hash ||
     url.username || url.password || url.origin!==url.href.replace(/\/$/,'')){
   throw Error('Некоректна HTTPS-адреса');
  }
 }catch{
  report('Введи HTTPS-адресу без шляху, параметрів або логіна: https://disk.sitezilla.net');
  return;
 }
 try{
  const info=await fetchJSON('/api/v1/setup/check',token);
  paint('sqliteState',info.database_ready?'Готова · '+info.device_count+' пристроїв':'Помилка',info.database_ready);
  paint('storageState',info.storage_writable?'Можна записувати':'Недоступне',info.storage_writable);
  $('quota').textContent=(info.quota_bytes/1073741824).toFixed(1)+' ГіБ (логічний ліміт)';
  $('freeDisk').textContent=(info.free_bytes/1073741824).toFixed(1)+' ГіБ';
  $('ttl').textContent=info.ttl_hours+' год';
  if(info.installed){lock();return}
  if(!info.database_ready||!info.storage_writable) {
   report('Виправ права на /var/lib/synchro або SQLite й повтори перевірку.');
   return;
  }
  $('next2').disabled=false;
  report('Сервер, база й право запису перевірені. Можна створювати пристрої.',true);
 }catch(e){
  paint('sqliteState','Недоступно',false);
  paint('storageState','Недоступно',false);
  report('Не вдалося виконати перевірку: '+e.message);
 }
}
async function finish() {
 if(installing)return;
 const token=$('adminKey').value.trim();
 const windows=$('windowsName').value.trim();
 const android=$('androidName').value.trim();
 if(!windows||!android||windows===android){
  report('Вкажи дві різні непорожні назви для пристроїв.');
  return;
 }
 if(!confirm('Завершити інсталяцію та видати ключі Windows/Android? Вебмайстер буде заблоковано.'))return;
 installing=true;$('finish').disabled=true;report('Створюємо облікові дані та блокуємо повторне встановлення…',true);
 try{
  const data=await fetchJSON('/api/v1/setup/finish',token,{
   public_url:$('domain').value.trim(),
   windows_name:windows,android_name:android
  });
  $('adminKey').value='';
  const root=$('credentials');root.replaceChildren();
  for(const dev of data.devices){
   const card=document.createElement('div');card.className='token';
   const name=document.createElement('h2');name.textContent='▣ '+dev.name;
   const detail=document.createElement('p');detail.textContent='Device ID: '+dev.id;
   const secret=document.createElement('div');secret.className='secret';secret.textContent=dev.token;
   const copy=document.createElement('button');copy.type='button';copy.textContent='⧉ Скопіювати токен';
   copy.addEventListener('click',async()=>{
    try{await navigator.clipboard.writeText(dev.token);copy.textContent='✓ Скопійовано'}
    catch{copy.textContent='Виділи й скопіюй токен вручну'}
   });
   card.append(name,detail,secret,copy);root.append(card);
  }
  report('Інсталяція завершена. Збережи ключі, перш ніж закрити сторінку!',true);
  show(4);
 }catch(e){
  report('Помилка інсталяції: '+e.message);
  // A connection drop after a committed transaction may mean the server completed
  // setup but the one-time credentials were not delivered. Do not auto-retry.
  $('finish').disabled=false;
 }finally{installing=false}
}
$('recheck').addEventListener('click',verifyFrontend);
$('next1').addEventListener('click',()=>{if(checked)show(2)});
$('back2').addEventListener('click',()=>{report('');show(1)});
$('runChecks').addEventListener('click',inspect);
$('next2').addEventListener('click',()=>{if(!$('next2').disabled){report('');show(3)}});
$('back3').addEventListener('click',()=>{report('');show(2)});
$('finish').addEventListener('click',finish);
$('domain').value=location.protocol==='https:'?location.origin:'';
verifyFrontend();
