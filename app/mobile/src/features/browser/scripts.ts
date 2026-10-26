// Scripts injected into pages (JavaScript strings) and WebView constants.
import { Platform } from 'react-native'

export const DESKTOP_USER_AGENT = Platform.select({
  ios: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15',
  default: 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36'
})

/**
 * After load: the page's icon `<link>` elements, sent to the app (which picks the best one, see `pickBestIcon` in the shared package).
 * Replayed (debounced by 400 ms) if the page changes its icons. `/favicon.ico` goes last, with a tiny size so it only wins when there is no other.
 */
export const META_SCRIPT = `(function(){try{
if(window.top!==window||window.__signetIcons)return;window.__signetIcons=1;
var send=function(){try{
var out=[],ls=document.querySelectorAll('link[rel]');
for(var i=0;i<ls.length&&out.length<30;i++){var l=ls[i],r=(l.getAttribute('rel')||'').toLowerCase();
if(!/(^|\\s)(icon|shortcut|apple-touch-icon|apple-touch-icon-precomposed)(\\s|$)/.test(r)||!l.href)continue;
out.push({href:l.href,rel:r,sizes:l.getAttribute('sizes')||'',type:l.getAttribute('type')||''});}
out.push({href:location.origin+'/favicon.ico',rel:'icon',sizes:'1x1'});
window.ReactNativeWebView.postMessage(JSON.stringify({type:'icons',base:document.baseURI,icons:out}));
}catch(e){}};
send();var t=0;
new MutationObserver(function(){clearTimeout(t);t=setTimeout(send,400);}).observe(document.head||document.documentElement,{childList:true,subtree:true,attributes:true,attributeFilter:['href','rel']});
}catch(e){}})();true;`

/**
 * Long press on a link (before load, in all frames): iOS's preview bubble is disabled for links only
 * (`-webkit-touch-callout: none` on `a[href]`); after ~450 ms without moving more than 10 px, the page sends `linkMenu` and the click that follows is swallowed.
 * Text selection and images behave as before.
 */
export const LINK_MENU_SCRIPT = `(function(){try{
if(window.__signetLink)return;window.__signetLink=1;
var css='a[href]{-webkit-touch-callout:none!important}';
var addCss=function(){try{var st=document.createElement('style');st.textContent=css;(document.head||document.documentElement).appendChild(st);}catch(e){}};
if(document.documentElement)addCss();else document.addEventListener('DOMContentLoaded',addCss);
var timer=0,sx=0,sy=0,link=null,fired=0;
var cancel=function(){clearTimeout(timer);timer=0;link=null;};
var find=function(n){while(n&&n!==document){if(n.nodeType===1&&n.tagName&&n.tagName.toLowerCase()==='a'&&n.hasAttribute('href'))return n;n=n.parentNode;}return null;};
var hrefOf=function(a){try{return new URL(a.getAttribute('href'),document.baseURI).href;}catch(e){return '';}};
document.addEventListener('touchstart',function(e){
cancel();if(e.touches.length!==1)return;
var a=find(e.target);if(!a)return;
var href=hrefOf(a);if(!/^https?:/i.test(href))return;
sx=e.touches[0].clientX;sy=e.touches[0].clientY;link=a;
timer=setTimeout(function(){timer=0;if(!link)return;
var img=link.querySelector('img'),text=(link.innerText||link.textContent||link.getAttribute('aria-label')||link.title||(img&&img.alt)||'').replace(/\\s+/g,' ').trim().slice(0,200);
fired=Date.now();
try{var sel=window.getSelection();if(sel)sel.removeAllRanges();}catch(e){}
window.ReactNativeWebView.postMessage(JSON.stringify({type:'linkMenu',href:href,text:text,x:sx,y:sy}));
link=null;},450);
},{passive:true,capture:true});
document.addEventListener('touchmove',function(e){
if(!timer)return;var t=e.touches[0];
if(Math.abs(t.clientX-sx)>10||Math.abs(t.clientY-sy)>10)cancel();
},{passive:true,capture:true});
document.addEventListener('touchend',function(){cancel();},{passive:true,capture:true});
document.addEventListener('touchcancel',function(){cancel();},{passive:true,capture:true});
document.addEventListener('contextmenu',function(e){if(Date.now()-fired<1000&&find(e.target))e.preventDefault();},true);
document.addEventListener('click',function(e){
if(fired&&Date.now()-fired<1200&&find(e.target)){fired=0;e.preventDefault();e.stopPropagation();e.stopImmediatePropagation();}
},true);
}catch(e){}})();true;`

/** Android only (iOS sends its scroll events natively): scroll position. */
export const SCROLL_SCRIPT = `(function(){try{
if(window.top!==window||window.__signetScroll)return;window.__signetScroll=1;var t=0;
window.addEventListener('scroll',function(){if(t)return;t=requestAnimationFrame(function(){t=0;
var e=document.scrollingElement||document.documentElement;
window.ReactNativeWebView.postMessage(JSON.stringify({type:'scroll',y:e.scrollTop,max:e.scrollHeight-e.clientHeight}));});},{passive:true});
}catch(e){}})();true;`

/** Page text size, in percent. */
export const zoomScript = (percent: number): string =>
  `(function(){try{var s=document.documentElement.style;s.webkitTextSizeAdjust='${percent}%';s.textSizeAdjust='${percent}%';}catch(e){}})();true;`

/** Address loaded by script (to reload an address identical to the source without remounting the WebView). */
export const locationScript = (url: string): string => `window.location.href=${JSON.stringify(url)};true;`

/** Android (no `contentMode`): forces a 1024 px wide layout when the desktop version is requested. */
export const DESKTOP_VIEWPORT_SCRIPT = `(function(){try{
var set=function(){var m=document.querySelector('meta[name=viewport]');
if(!m){m=document.createElement('meta');m.name='viewport';(document.head||document.documentElement).appendChild(m);}
m.setAttribute('content','width=1024');};
if(document.head)set();document.addEventListener('DOMContentLoaded',set);
}catch(e){}})();true;`
