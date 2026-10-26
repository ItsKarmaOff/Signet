import type { Browser } from '../browser/browser'
import { showSpaceMenu } from '../app/menu'
import type { Registrar } from './registrar'

/** Spaces and routing rules. */
export function registerSpacesIpc(b: Browser, { handle }: Registrar): void {
  // From the Library, choosing a space brings the main window to the front.
  handle('switchSpace', ({ sender }, id) => {
    b.spaces.hint(id, sender)
    b.spaces.switchTo(id)
    if (b.library.owns(sender)) b.window.show()
  })
  handle('createSpace', (_c, draft) => b.spaces.create(draft))
  handle('updateSpace', (_c, id, draft) => b.spaces.update(id, draft))
  // From the Library, "Edit" opens the space form directly.
  handle('showSpaceMenu', ({ sender }, id) => (b.library.owns(sender) ? b.library.editSpace(id) : showSpaceMenu(b, id)))
  handle('setSpaceProfile', (_c, id, profile) => b.spaces.setProfile(id, profile))
  handle('addRoute', (_c, pattern, spaceId) => b.spaces.addRoute(pattern, spaceId))
  handle('removeRoute', (_c, id) => b.spaces.removeRoute(id))
}
