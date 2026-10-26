import { test, expect, openTab } from './fixtures'

const bar = (a: Parameters<typeof openTab>[0]) =>
  a.signet((b) => (b.space.bookmarks ?? []).map((i) => (i.kind === 'link' ? i.url : `[${i.name}: ${i.items.length}]`)))

test('bookmarks: add, remove, drop, folder, open and bar height', async ({ signetApp, site }) => {
  const t1 = await openTab(signetApp, site.url('un'), 'un')
  const t2 = await openTab(signetApp, site.url('deux'), 'deux')
  const t3 = await openTab(signetApp, site.url('trois'), 'trois')

  // ⌘D: adds, then removes on an already bookmarked page.
  await signetApp.signet((b, ids) => ids.slice(0, 2).forEach((id) => b.bookmarks.toggle(id)), [t1, t2])
  expect(await bar(signetApp)).toEqual([site.url('un'), site.url('deux')])
  await signetApp.signet((b, id) => b.bookmarks.toggle(id), t2)
  expect(await bar(signetApp)).toEqual([site.url('un')])

  // Tab dropped at the start of the bar.
  await signetApp.signet((b, id) => b.bookmarks.add(id, { index: 0 }), t3)
  expect((await bar(signetApp))[0]).toBe(site.url('trois'))

  // Filing into a folder; a folder is never filed into itself.
  const moved = await signetApp.signet((b) => {
    const folder = { kind: 'folder' as const, id: 'dossier-test', name: 'Lectures', items: [] }
    b.space.bookmarks!.push(folder)
    b.bookmarks.move(b.space.bookmarks![0].id, { folderId: folder.id, index: 0 })
    b.bookmarks.move(folder.id, { folderId: folder.id, index: 0 })
    return { inside: folder.items.length, root: b.space.bookmarks!.length, kept: b.space.bookmarks!.includes(folder) }
  })
  expect(moved).toEqual({ inside: 1, root: 2, kept: true })

  // Opens in the given tab.
  const target = await signetApp.signet((b, id) => {
    const link = b.space.bookmarks!.find((i) => i.kind === 'link')!
    b.bookmarks.open(link.id, id, false)
    return link.kind === 'link' ? link.url : ''
  }, t1)
  await expect.poll(async () => (await signetApp.snapshot()).tabs[t1]?.url).toBe(target)

  // ⌘⇧B: the web card regains the bar's height (32 px).
  const card = () => signetApp.signet((b) => b.broadcast.snapshot().panes[0]?.rect)
  const withBar = await card()
  await signetApp.signet((b) => b.bookmarks.toggleBar())
  await expect.poll(async () => (await card())?.height).toBe(withBar!.height + 32)
  expect((await card())!.y).toBe(withBar!.y - 32)
  await signetApp.signet((b) => b.bookmarks.toggleBar())
  await expect.poll(card).toEqual(withBar)
})
