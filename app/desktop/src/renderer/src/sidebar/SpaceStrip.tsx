import { useLayoutEffect, useRef, useState } from 'react'
import type { ReactNode } from 'react'
import { useSpaceSwipe } from '../lib/useSpaceSwipe'

/** Width of a space icon in the bottom bar. */
const SLOT = 28
/** At most 5 icons per page, 4 when space is tight. */
const perPageFor = (width: number) => (width >= 5 * SLOT ? 5 : 4)

/**
 * Space icons at the bottom of the sidebar, arranged in pages when they do not fit.
 * Pages are dragged on the trackpad like spaces; switching space brings its page on screen.
 */
export function SpaceStrip({ activeIndex, children }: { activeIndex: number; children: ReactNode[] }) {
  const rootRef = useRef<HTMLDivElement>(null)
  const [perPage, setPerPage] = useState(5)

  useLayoutEffect(() => {
    const el = rootRef.current
    if (!el) return
    const measure = () => setPerPage(perPageFor(el.clientWidth))
    measure()
    const ro = new ResizeObserver(measure)
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  const pages: ReactNode[][] = []
  for (let i = 0; i < children.length; i += perPage) pages.push(children.slice(i, i + perPage))
  // By default, the active space's page; a page chosen by gesture holds until the next space change.
  const [picked, setPicked] = useState<{ page: number; active: number; perPage: number } | null>(null)
  const own = picked && picked.active === activeIndex && picked.perPage === perPage ? picked.page : Math.floor(Math.max(0, activeIndex) / perPage)
  const shown = Math.min(own, pages.length - 1)
  // A change in the number of icons per page (measurement, resize) recenters without sliding.
  const lastPerPage = useRef(perPage)
  useLayoutEffect(() => {
    lastPerPage.current = perPage
  })

  useSpaceSwipe({
    rootRef,
    clipRef: rootRef,
    track: '.strip-track',
    animateJumps: lastPerPage.current === perPage,
    spaceIds: pages.map((_, i) => String(i)),
    activeId: String(shown),
    onSwitch: (id) => setPicked({ page: Number(id), active: activeIndex, perPage })
  })

  return (
    <div ref={rootRef} className="space-strip" data-prev={shown > 0 ? '' : undefined} data-next={shown < pages.length - 1 ? '' : undefined}>
      <div className="strip-track" style={{ ['--page' as string]: shown }}>
        {pages.map((p, i) => (
          <div key={i} className="strip-page">
            {p}
          </div>
        ))}
      </div>
    </div>
  )
}
