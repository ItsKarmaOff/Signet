// Scripts injected into pages (JavaScript strings): login form detection, capture of what is submitted, filling.
// They neither receive nor keep any password: capture posts it to the app at submit time, filling receives it at call time.
// Messages from the page are untrusted data: the app checks the origin against the tab's real address.

/** Common functions (field detection), shared by both scripts. */
const COMMON = `
var vis=function(el){try{return (el.offsetWidth||el.offsetHeight||el.getClientRects().length)>0&&getComputedStyle(el).visibility!=='hidden'}catch(e){return false}};
var auto=function(el){return (el.getAttribute('autocomplete')||'').toLowerCase()};
var textLike=function(el){return el.tagName==='INPUT'&&['text','email','tel',''].indexOf(el.type)>=0&&!el.readOnly&&!el.disabled};
var HINT=/user|login|identifier|account|e-?mail|courriel|identifiant/i;
var rootOf=function(el){if(el.form)return el.form;var n=el.parentElement;
for(var i=0;n&&i<8;i++,n=n.parentElement)if(n.querySelector('input[type="password"]'))return n;return document};
var pwsOf=function(r){return [].filter.call(r.querySelectorAll('input[type="password"]'),vis)};
var userOf=function(r,pw){var ins=[].filter.call(r.querySelectorAll('input'),vis);
var tagged=ins.filter(function(i){return textLike(i)&&/\\b(username|email)\\b/.test(auto(i))})[0];if(tagged)return tagged;
var before=pw?ins.slice(0,ins.indexOf(pw)):ins;return before.reverse().filter(textLike)[0]||null};
`

/** After loading: reports whether there is a login form, and posts the submitted credentials. */
export const PASSWORD_SCRIPT = `(function(){try{
if(window.__signetPw||!/^https?:$/.test(location.protocol))return;window.__signetPw=1;
var post=function(o){try{window.ReactNativeWebView.postMessage(JSON.stringify(o))}catch(e){}};
${COMMON}
var lastHas=null,lastHref='',timer=0;
var scan=function(){timer=0;var has=pwsOf(document).some(function(p){return auto(p)!=='new-password'});
if(has!==lastHas||location.href!==lastHref){lastHas=has;lastHref=location.href;post({type:'pw:form',has:has})}};
var later=function(){if(!timer)timer=setTimeout(scan,400)};
scan();
new MutationObserver(later).observe(document.documentElement,{childList:true,subtree:true});
addEventListener('popstate',later);addEventListener('hashchange',later);
document.addEventListener('focusin',later,true);
var chosen=function(r){var f=pwsOf(r).filter(function(p){return p.value});if(!f.length)return null;
var fresh=f.filter(function(p){return auto(p)==='new-password'});if(fresh.length)return fresh[0];
var twice=f.filter(function(p,i){return f.some(function(q,j){return j!==i&&q.value===p.value})})[0];if(twice)return twice;
return f.length===3?f[1]:f[0]};
var lastKey='',lastAt=0;
var report=function(from){var r=rootOf(from),pw=chosen(r),username;
if(pw){var u=userOf(r,pw);username=u?u.value.trim():''}
else{var id=(from.tagName==='INPUT'&&textLike(from))?from:userOf(r,null);
if(!id||!id.value.trim()||!(/\\b(username|email)\\b/.test(auto(id))||HINT.test(id.name+' '+id.id+' '+id.type)))return;username=id.value.trim()}
var p=pw?pw.value:'',key=username+'\\n'+p;
if(key===lastKey&&Date.now()-lastAt<2000)return;lastKey=key;lastAt=Date.now();
post({type:'pw:submit',origin:location.origin,username:username,password:p})};
document.addEventListener('submit',function(e){if(!e.isTrusted&&!(navigator.userActivation&&navigator.userActivation.isActive))return;
if(e.target instanceof HTMLFormElement)report(e.target)},true);
document.addEventListener('keydown',function(e){if(e.key==='Enter'&&e.isTrusted&&e.target&&e.target.tagName==='INPUT'&&(e.target.type==='password'||textLike(e.target)))report(e.target)},true);
document.addEventListener('click',function(e){if(!e.isTrusted||!(e.target instanceof Element))return;
var b=e.target.closest('button,input[type="submit"],input[type="image"],[role="button"]');if(!b)return;
if(b.tagName==='BUTTON'&&b.type!=='submit'&&b.form)return;
var r=rootOf(b);if(r!==document)return report(b);
var ps=document.querySelectorAll('input[type="password"]');for(var i=0;i<ps.length;i++)if(ps[i].value&&vis(ps[i]))return report(b)},true);
}catch(e){}})();true;`

/** Fills the page's form, provided it is still on `origin` (the page may have changed during authentication). */
export const fillScript = (
  origin: string,
  username: string,
  password: string
): string => `(function(o,u,p){var res=function(ok){try{window.ReactNativeWebView.postMessage(JSON.stringify({type:'pw:filled',ok:ok}))}catch(e){}};
try{if(location.origin!==o)return res(false);
${COMMON}
var set=function(el,v){var d=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value');
if(d&&d.set)d.set.call(el,v);else el.value=v;
el.dispatchEvent(new Event('input',{bubbles:true}));el.dispatchEvent(new Event('change',{bubbles:true}))};
var pws=pwsOf(document),pw=pws.filter(function(x){return auto(x)!=='new-password'})[0]||null;
var user=pw?userOf(rootOf(pw),pw):userOf(document,null);
if(!pw&&!(user&&HINT.test(user.name+' '+user.id+' '+auto(user)+' '+user.type)))return res(false);
if(user&&u)set(user,u);
if(pw){set(pw,p);pw.focus()}
res(true)}catch(e){res(false)}})(${JSON.stringify(origin)},${JSON.stringify(username)},${JSON.stringify(password)});true;`
