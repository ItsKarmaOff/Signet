(() => {
  __READERABLE__
  let readerable = false
  try { readerable = isProbablyReaderable(document) } catch {}
  return { readerable, lang: document.documentElement.lang || '' }
})()
