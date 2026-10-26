// Reader mode: an extraction script injected into the page (compact heuristic, no dependency), reading its message,
// and the clean HTML page that shows the article. Display happens in a separate WebView with JavaScript off.

export interface ReaderArticle {
  title: string
  byline: string
  siteName: string
  /** HTML already rebuilt by the script (text tags, http(s) links and images only). */
  html: string
  url: string
  /** Text length, to estimate reading time. */
  length: number
}

export type ReaderTheme = 'light' | 'sepia' | 'dark'

export interface ReaderPrefs {
  /** Text size in points. */
  size: number
  theme: ReaderTheme
  serif: boolean
}

export const READER_MIN_SIZE = 14
export const READER_MAX_SIZE = 28
export const READER_SIZE_STEP = 2

export const defaultReaderPrefs = (scheme: 'light' | 'dark'): ReaderPrefs => ({ size: 19, theme: scheme === 'dark' ? 'dark' : 'light', serif: true })

export function sanitizeReaderPrefs(raw: unknown, base: ReaderPrefs): ReaderPrefs {
  const r = (raw && typeof raw === 'object' ? raw : {}) as Record<string, unknown>
  const size = typeof r.size === 'number' && Number.isFinite(r.size) ? Math.min(READER_MAX_SIZE, Math.max(READER_MIN_SIZE, Math.round(r.size))) : base.size
  const theme = r.theme === 'light' || r.theme === 'sepia' || r.theme === 'dark' ? r.theme : base.theme
  return { size, theme, serif: typeof r.serif === 'boolean' ? r.serif : base.serif }
}

/** WebView message: the article, or `article: null` when the page has none. */
export function parseReaderMessage(data: string): { article: ReaderArticle | null } | null {
  try {
    const m = JSON.parse(data) as { type?: string; article?: Partial<ReaderArticle> | null } | null
    if (!m || m.type !== 'reader') return null
    const a = m.article
    if (!a || typeof a.html !== 'string' || !a.html) return { article: null }
    return {
      article: {
        title: String(a.title ?? ''),
        byline: String(a.byline ?? ''),
        siteName: String(a.siteName ?? ''),
        html: a.html,
        url: String(a.url ?? ''),
        length: typeof a.length === 'number' ? a.length : 0
      }
    }
  } catch {
    return null
  }
}

export const escapeHtml = (s: string): string =>
  s.replace(/[&<>"']/g, (ch) => (ch === '&' ? '&amp;' : ch === '<' ? '&lt;' : ch === '>' ? '&gt;' : ch === '"' ? '&quot;' : '&#39;'))

/** Reading minutes (about 220 words per minute, 6 characters per word), at least 1. */
export const readingMinutes = (length: number): number => Math.max(1, Math.round(length / 6 / 220))

/** Palette of each reading theme. */
export const READER_PALETTE: Record<ReaderTheme, { bg: string; text: string; secondary: string; link: string; rule: string; code: string }> = {
  light: { bg: '#ffffff', text: '#1d1d1f', secondary: '#6c6c73', link: '#0a63d6', rule: 'rgba(0,0,0,0.12)', code: 'rgba(0,0,0,0.05)' },
  sepia: { bg: '#f4ecd8', text: '#433422', secondary: '#7d6b53', link: '#8a4b12', rule: 'rgba(67,52,34,0.18)', code: 'rgba(67,52,34,0.07)' },
  dark: { bg: '#17171a', text: '#e6e6ea', secondary: '#9a99a3', link: '#6db0ff', rule: 'rgba(255,255,255,0.14)', code: 'rgba(255,255,255,0.07)' }
}

/** Full HTML page of the article. `minutesLabel`: already translated text ("4 min read"). */
export function buildReaderHtml(article: ReaderArticle, prefs: ReaderPrefs, minutesLabel = ''): string {
  const p = READER_PALETTE[prefs.theme]
  const font = prefs.serif ? 'Georgia, "Iowan Old Style", "Times New Roman", serif' : '-apple-system, "SF Pro Text", Roboto, "Helvetica Neue", sans-serif'
  const meta = [article.siteName, article.byline, minutesLabel].filter(Boolean).map(escapeHtml).join(' · ')
  const css = `
html{-webkit-text-size-adjust:100%;background:${p.bg}}
body{margin:0;padding:24px 22px 96px;background:${p.bg};color:${p.text};font:${prefs.size}px/1.6 ${font};max-width:720px;margin-inline:auto;word-wrap:break-word;overflow-wrap:anywhere}
h1.t{font:700 ${Math.round(prefs.size * 1.55)}px/1.2 -apple-system,"SF Pro Display",Roboto,sans-serif;margin:8px 0 10px;letter-spacing:-0.01em}
.m{font:500 ${Math.max(12, prefs.size - 6)}px/1.4 -apple-system,Roboto,sans-serif;color:${p.secondary};margin:0 0 24px;padding-bottom:18px;border-bottom:1px solid ${p.rule}}
h1,h2,h3,h4,h5,h6{font-family:-apple-system,"SF Pro Display",Roboto,sans-serif;line-height:1.25;margin:1.6em 0 .5em}
h1{font-size:1.5em}h2{font-size:1.3em}h3{font-size:1.15em}h4,h5,h6{font-size:1em}
p{margin:0 0 1.1em}a{color:${p.link};text-decoration:underline;text-decoration-thickness:1px;text-underline-offset:2px}
img{max-width:100%;height:auto;border-radius:8px;display:block;margin:1em auto}
figure{margin:1.2em 0}figcaption{font:${Math.max(12, prefs.size - 5)}px/1.4 -apple-system,Roboto,sans-serif;color:${p.secondary};text-align:center}
blockquote{margin:1.2em 0;padding:.1em 0 .1em 1em;border-left:3px solid ${p.rule};color:${p.secondary}}
pre,code{font:${Math.max(12, prefs.size - 4)}px/1.5 ui-monospace,Menlo,monospace;background:${p.code};border-radius:6px}
code{padding:.1em .3em}pre{padding:12px;overflow-x:auto}pre code{padding:0;background:none}
table{border-collapse:collapse;display:block;overflow-x:auto}td,th{border:1px solid ${p.rule};padding:6px 10px}
hr{border:0;border-top:1px solid ${p.rule};margin:2em 0}ul,ol{padding-left:1.4em}li{margin:.3em 0}`
  return `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src https: http: data:; style-src 'unsafe-inline'"><meta name="referrer" content="no-referrer"><style>${css}</style></head><body><h1 class="t">${escapeHtml(article.title)}</h1>${meta ? `<p class="m">${meta}</p>` : ''}${article.html}</body></html>`
}

/** Extraction script: returns `{ type: 'reader', article }` (or `article: null`) via `postMessage`. */
export const READER_EXTRACT_SCRIPT = `(function(){
try{
var d=document,loc=location;
var txt=function(n){return (n.textContent||'').replace(/\\s+/g,' ').trim()};
var esc=function(s){return s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;')};
var meta=function(sel){var e=d.querySelector(sel);return e?(e.getAttribute('content')||e.textContent||'').trim():''};
var UNLIKELY=/(comment|combx|disqus|footer|menu|nav(bar|igation)?\\b|promo|related|share|social|sidebar|sponsor|subscribe|newsletter|cookie|advert|banner|popup|modal|breadcrumb|toolbar|widget)/i;
var scores=new Map(),best=null,bestScore=0;
var ps=d.querySelectorAll('p,pre');
for(var i=0;i<ps.length;i++){
  var p=ps[i];if(!p.getClientRects().length)continue;var t=txt(p);if(t.length<50)continue;
  var par=p.parentElement;if(!par||UNLIKELY.test((par.className||'')+' '+(par.id||'')))continue;
  var s=1+Math.min(3,Math.floor(t.length/100))+(t.split(',').length-1)*0.3;
  var lvl=0;
  for(var a=par;a&&a!==d.documentElement&&lvl<3;a=a.parentElement,lvl++){scores.set(a,(scores.get(a)||0)+s/(lvl===0?1:lvl*2));}
}
scores.forEach(function(v,el){
  var tag=el.tagName,bonus=(tag==='ARTICLE'||tag==='MAIN'||el.getAttribute('role')==='main'||el.getAttribute('itemprop')==='articleBody')?1.3:1;
  var linkLen=0,as=el.querySelectorAll('a');for(var k=0;k<as.length;k++)linkLen+=txt(as[k]).length;
  var total=txt(el).length||1;var density=linkLen/total;
  var sc=v*bonus*(1-Math.min(0.9,density));
  if(sc>bestScore){bestScore=sc;best=el;}
});
if(!best||txt(best).length<400){window.ReactNativeWebView.postMessage(JSON.stringify({type:'reader',article:null}));return;}
var title=meta('meta[property="og:title"]')||(d.querySelector('h1')?txt(d.querySelector('h1')):'')||d.title||'';
var SKIP={SCRIPT:1,STYLE:1,NOSCRIPT:1,NAV:1,ASIDE:1,FORM:1,IFRAME:1,BUTTON:1,FOOTER:1,SVG:1,INPUT:1,SELECT:1,TEXTAREA:1,VIDEO:1,AUDIO:1,CANVAS:1,OBJECT:1,EMBED:1,DIALOG:1,TEMPLATE:1,LINK:1,META:1};
var KEEP={P:1,H1:1,H2:1,H3:1,H4:1,H5:1,H6:1,UL:1,OL:1,LI:1,BLOCKQUOTE:1,PRE:1,CODE:1,EM:1,I:1,STRONG:1,B:1,SUB:1,SUP:1,FIGURE:1,FIGCAPTION:1,TABLE:1,THEAD:1,TBODY:1,TR:1,TD:1,TH:1,DEL:1,MARK:1,U:1,S:1};
var abs=function(u){try{var x=new URL(u,loc.href);return /^https?:$/.test(x.protocol)?x.href:''}catch(e){return ''}};
var count=0;
var walk=function(n,depth){
  if(depth>60)return '';
  if(n.nodeType===3)return esc(n.nodeValue||'');
  if(n.nodeType!==1)return '';
  var tag=n.tagName;
  if(SKIP[tag])return '';
  if(n.hasAttribute('hidden')||n.getAttribute('aria-hidden')==='true')return '';
  var cs=null;try{cs=getComputedStyle(n)}catch(e){}
  if(cs&&(cs.display==='none'||cs.visibility==='hidden'))return '';
  if(n!==best&&UNLIKELY.test((n.className&&n.className.baseVal===undefined?n.className:'')+' '+(n.id||''))&&txt(n).length<600)return '';
  if(tag==='IMG'){
    var src=abs(n.currentSrc||n.getAttribute('src')||n.getAttribute('data-src')||'');
    if(!src||(n.naturalWidth&&n.naturalWidth<80))return '';
    count++;return '<img src="'+esc(src)+'" alt="'+esc(n.getAttribute('alt')||'')+'">';
  }
  if(tag==='BR')return '<br>';
  if(tag==='HR')return '<hr>';
  if(tag==='PICTURE'){var im=n.querySelector('img');return im?walk(im,depth+1):'';}
  var inner='';for(var c=n.firstChild;c;c=c.nextSibling)inner+=walk(c,depth+1);
  if(tag==='H1'&&txt(n)===title)return '';
  if(tag==='A'){var h=abs(n.getAttribute('href')||'');return h&&inner.trim()?'<a href="'+esc(h)+'">'+inner+'</a>':inner;}
  if(KEEP[tag]){
    if(!inner.trim())return '';
    var l=tag.toLowerCase();return '<'+l+'>'+inner+'</'+l+'>';
  }
  if(tag==='DIV'||tag==='SECTION'||tag==='ARTICLE'||tag==='MAIN'||tag==='SPAN'||tag==='HEADER'){
    if(tag==='HEADER'&&n!==best&&txt(n).length<200)return '';
    return inner.trim()?(tag==='SPAN'?inner:'<div>'+inner+'</div>'):'';
  }
  return inner;
};
var html=walk(best,0);
var len=txt(best).length;
if(!html||len<400){window.ReactNativeWebView.postMessage(JSON.stringify({type:'reader',article:null}));return;}
var byline=meta('meta[name="author"]')||meta('meta[property="article:author"]')||meta('[rel="author"]')||meta('[itemprop="author"]')||meta('.byline');
if(byline.length>80)byline='';
var site=meta('meta[property="og:site_name"]')||loc.hostname.replace(/^www\\./,'');
window.ReactNativeWebView.postMessage(JSON.stringify({type:'reader',article:{title:title,byline:byline,siteName:site,html:html,url:loc.href,length:len}}));
}catch(e){try{window.ReactNativeWebView.postMessage(JSON.stringify({type:'reader',article:null}))}catch(_){}}
})();true;`
