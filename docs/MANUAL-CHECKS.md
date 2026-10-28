# Manual checks

What could not be tested automatically: it needs a real mouse, a real trackpad, a real keyboard or a real account. Check items off as you go.

## Editable shortcuts (Settings › Shortcuts)

- [ ] Change a shortcut (for example "Open a split view", in the "Split view" group, to ⌘E), then use it: the command runs.
- [ ] While a shortcut is being recorded, ⌘W doesn't close the window and ⌘Q doesn't quit.
- [ ] After "Reset all", the original shortcuts work again.

## Live folders

- [ ] Settings › General › Live folders: save a GitHub token (scope `repo`).
- [ ] Create a "GitHub › Reviews requested from me" folder and a "My open pull requests" one: they fill up.
- [ ] Leave Signet open for more than 15 minutes: a feed folder updates on its own.

## Notes and Easels

- [ ] In an Easel, scroll with two fingers and pinch to zoom on the trackpad.
- [ ] Drag an image from the Finder into an Easel, and paste a copied image.
- [ ] In a note, ⌘K on a selected word creates a link.

## Backup (Settings › Backup)

- [ ] Export with passwords: Touch ID is requested, the `.signetbackup` file is created.
- [ ] Import this file in "Add to what's there" mode: the confirmation shows, Signet restarts, nothing is duplicated.
- [ ] Import in "Replace what's there" mode, then restore the safety copy from the same section.
- [ ] Import a backup that contains extensions: they reinstall on restart.

## Passwords: key button

- [ ] Sign in to a site, click elsewhere when the offer to save shows up: the key stays in the address bar, and a click reopens the offer.
- [ ] On a site that is already saved, the key lists the logins; "Copy" puts the username on the clipboard.

## Appearance

- [ ] A gradient space with grain: the grain is visible to the eye.
- [ ] The hover preview doesn't show for the current tab, nor for the panes of a split view.

## Interface that disappears

- [ ] If the sidebar and the toolbar disappear: the interface should come back on its own. If not, View › Reload Interface.
- [ ] After such an incident, send the file `~/Library/Application Support/Signet/logs/interface.log`: it says what crashed.

## Quit, mini player, extensions

- [ ] Press ⌘Q once: the "Hold ⌘Q to quit" bubble shows up; holding ⌘Q quits the app.
- [ ] Play a YouTube video, then switch tabs: the mini player shows up; it can be moved and resized with the mouse.
- [ ] Drag a pinned extension button in the toolbar to change its place; a click still opens the extension's window.

## Passkeys from a phone (Linux; macOS in development with `SIGNET_WEBAUTHN_INTERCEPT=1`)

- [ ] On webauthn.io, "Register": the "Passkey" window opens with a QR code; "Cancel", Esc and closing the window hand control back to the site (error).
- [ ] Bluetooth turned off on the computer: the window says so; once it is back on, the normal message returns.
- [ ] Scan the QR code with a phone (iPhone or Android), accept: "Phone found. Connecting…", then "Confirm on your phone.", then the window closes and webauthn.io confirms the registration.
- [ ] "Authenticate" on webauthn.io with the passkey you created: the sign-in succeeds.
- [ ] Microsoft sign-in with a passkey from the phone; without a phone, "Cancel" leads to "Use my password".
- [ ] The same thing in a private window: the QR code window opens above the private window.
- [ ] Linux: check first (the Mac has already been validated) that BlueZ detects the phone; if it fails, note the `Passkey:` and `Bluetooth:` lines from the terminal (`npm run dev`).
