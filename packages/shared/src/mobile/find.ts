// In-page search: scripts injected into the WebView. Matches are wrapped in <mark data-signet-find>;
// the script replies with a `{ type: 'find', count, index }` message (index starts at 0, -1 if nothing found).

/** Escapes user input so it can be used as is in a regular expression. */
export const escapeRegExp = (s: string): string => s.replace(/[.*+?^${}()|[\]\\/-]/g, '\\$&')

export interface FindResult {
  type: 'find'
  count: number
  index: number
}

/** Reads a WebView message; `null` if it is not a search result. */
export function parseFindMessage(data: string): FindResult | null {
  try {
    const m = JSON.parse(data) as Partial<FindResult> | null
    if (m && m.type === 'find' && typeof m.count === 'number' && typeof m.index === 'number') return { type: 'find', count: m.count, index: m.index }
  } catch {
    // message of another type
  }
  return null
}

/** Maximum number of highlighted matches (beyond that, the page would become slow). */
export const MAX_FIND_MATCHES = 500

const RUNTIME = `
if(!window.__signetFind){
  var marks=[],cur=-1;
  var CSS='background:#ffe14d;color:#000;border-radius:2px;';
  var CUR='background:#ff9a1f;color:#000;border-radius:2px;';
  var post=function(){try{window.ReactNativeWebView.postMessage(JSON.stringify({type:'find',count:marks.length,index:marks.length?cur:-1}))}catch(e){}};
  var clear=function(){
    for(var i=0;i<marks.length;i++){var m=marks[i],p=m.parentNode;if(!p)continue;p.replaceChild(document.createTextNode(m.textContent),m);p.normalize();}
    marks=[];cur=-1;
  };
  var show=function(scroll){
    for(var i=0;i<marks.length;i++)marks[i].style.cssText=i===cur?CUR:CSS;
    if(scroll&&marks[cur])marks[cur].scrollIntoView({block:'center',inline:'nearest',behavior:'smooth'});
  };
  window.__signetFind={
    run:function(q,re,max){
      clear();
      if(!q){post();return;}
      var rx=new RegExp(re,'gi'),nodes=[],w=document.createTreeWalker(document.body,NodeFilter.SHOW_TEXT,{acceptNode:function(n){
        var p=n.parentNode;if(!p||!n.nodeValue||!n.nodeValue.trim())return NodeFilter.FILTER_REJECT;
        var t=p.nodeName;if(t==='SCRIPT'||t==='STYLE'||t==='NOSCRIPT'||t==='TEXTAREA'||t==='MARK'&&p.hasAttribute('data-signet-find'))return NodeFilter.FILTER_REJECT;
        var s=p.closest&&p.closest('[hidden],[aria-hidden="true"]');if(s)return NodeFilter.FILTER_REJECT;
        return NodeFilter.FILTER_ACCEPT;}});
      while(w.nextNode())nodes.push(w.currentNode);
      for(var i=0;i<nodes.length&&marks.length<max;i++){
        var n=nodes[i],text=n.nodeValue,last=0,m,frag=null;rx.lastIndex=0;
        while((m=rx.exec(text))&&m[0].length&&marks.length<max){
          frag=frag||document.createDocumentFragment();
          if(m.index>last)frag.appendChild(document.createTextNode(text.slice(last,m.index)));
          var mk=document.createElement('mark');mk.setAttribute('data-signet-find','');mk.textContent=m[0];frag.appendChild(mk);marks.push(mk);last=m.index+m[0].length;
        }
        if(frag){if(last<text.length)frag.appendChild(document.createTextNode(text.slice(last)));n.parentNode.replaceChild(frag,n);}
      }
      cur=marks.length?0:-1;show(true);post();
    },
    step:function(d){if(!marks.length){post();return;}cur=(cur+d+marks.length)%marks.length;show(true);post();},
    clear:function(){clear();}
  };
}`

const wrap = (call: string) => `(function(){try{${RUNTIME}${call}}catch(e){}})();true;`

/** Script that looks for `query` (case-insensitive) and highlights the matches. */
export const findScript = (query: string): string =>
  wrap(`window.__signetFind.run(${JSON.stringify(query)},${JSON.stringify(escapeRegExp(query))},${MAX_FIND_MATCHES});`)

/** Script that moves to the next (`1`) or previous (`-1`) match. */
export const findStepScript = (direction: 1 | -1): string => wrap(`window.__signetFind.step(${direction});`)

/** Script that removes all highlights. */
export const findClearScript = (): string => wrap('window.__signetFind.clear();')
