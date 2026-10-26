// Contract shared between the main process, the preload and the UI.
// Any data crossing IPC must be described in one of the files in this folder (one per domain);
// this index re-exports them all, so always import from '@shared/types'.
// The data model (tabs, spaces, settings...) lives in the @signet/shared package and is re-exported here:
// desktop code keeps importing all its types from '@shared/types'.
export * from '@signet/shared/types'

export * from './api'
export * from './data'
export * from './devtools'
export * from './extensions'
export * from './link'
export * from './mods'
export * from './overlay'
export * from './passwords'
export * from './permissions'
export * from './settings'
export * from './state'
export * from './tabs'
export * from './windows'
