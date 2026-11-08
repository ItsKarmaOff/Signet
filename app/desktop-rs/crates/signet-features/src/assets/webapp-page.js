(() => {
  const abs = (h) => { try { return new URL(h, document.baseURI).href } catch { return '' } }
  const icons = [...document.querySelectorAll('link[rel][href]')]
    .filter((l) => /(^|\s)(icon|apple-touch-icon|apple-touch-icon-precomposed)(\s|$)/i.test(l.rel))
    .slice(0, 40)
    .map((l) => ({ href: abs(l.getAttribute('href')), sizes: l.getAttribute('sizes') || '', rel: l.rel.toLowerCase(), type: l.type || '' }))
  const manifest = document.querySelector('link[rel~="manifest"][href]')
  const meta = (n) => (document.querySelector('meta[name="' + n + '"]')?.content || '').trim()
  return {
    manifest: manifest ? abs(manifest.getAttribute('href')) : null,
    icons,
    appName: meta('application-name') || meta('apple-mobile-web-app-title'),
    themeColor: meta('theme-color'),
    title: document.title || ''
  }
})()
