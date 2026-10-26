import { execFile } from 'node:child_process'
import { systemPreferences } from 'electron'

export type AuthResult = 'ok' | 'denied' | 'unavailable'

const TIMEOUT = 60_000

/** Runs an authentication script; any failure (error, timeout, exit code) = refusal. */
function run(file: string, args: string[], env: NodeJS.ProcessEnv = {}): Promise<AuthResult> {
  return new Promise((resolve) => {
    try {
      execFile(file, args, { timeout: TIMEOUT, windowsHide: true, maxBuffer: 64 * 1024, env: { ...process.env, ...env } }, (err, stdout) => {
        if (err) return resolve('denied')
        const out = String(stdout).trim().split(/\r?\n/).pop()
        resolve(out === 'ok' ? 'ok' : out === 'unavailable' ? 'unavailable' : 'denied')
      })
    } catch {
      resolve('denied')
    }
  })
}

/** macOS without Touch ID: account password via LocalAuthentication (LAPolicyDeviceOwnerAuthentication = 2). */
const JXA = `
ObjC.import('LocalAuthentication');
ObjC.import('Foundation');
function run(argv) {
  try {
    var ctx = $.LAContext.alloc.init;
    var err = Ref();
    if (!ctx.canEvaluatePolicyError(2, err)) return 'unavailable';
    var state = 'pending';
    ctx.evaluatePolicyLocalizedReasonReply(2, argv[0], function (ok, e) { state = ok ? 'ok' : 'denied'; });
    var until = Date.now() + 55000;
    while (state === 'pending' && Date.now() < until) {
      $.NSRunLoop.currentRunLoop.runUntilDate($.NSDate.dateWithTimeIntervalSinceNow(0.2));
    }
    return state === 'pending' ? 'denied' : state;
  } catch (e) {
    return 'denied';
  }
}
`

/** Windows Hello / PIN / password via UserConsentVerifier (WinRT, awaited with AsTask). */
const POWERSHELL = `
$ErrorActionPreference = 'Stop'
try {
  Add-Type -AssemblyName System.Runtime.WindowsRuntime
  $uc = [Windows.Security.Credentials.UI.UserConsentVerifier, Windows.Security.Credentials.UI, ContentType = WindowsRuntime]
  $asTask = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
    $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation\`1'
  })[0]
  function Await($op, $type) {
    $task = $asTask.MakeGenericMethod($type).Invoke($null, @($op))
    $task.Wait(-1) | Out-Null
    $task.Result
  }
  $avail = Await ($uc::CheckAvailabilityAsync()) ([Windows.Security.Credentials.UI.UserConsentVerifierAvailability])
  if ($avail.ToString() -ne 'Available') { 'unavailable'; exit 0 }
  $res = Await ($uc::RequestVerificationAsync($env:SIGNET_AUTH_REASON)) ([Windows.Security.Credentials.UI.UserConsentVerificationResult])
  if ($res.ToString() -eq 'Verified') { 'ok' } else { 'denied' }
} catch {
  'denied'
}
`

/** Asks the user to authenticate with the system (Touch ID, password, Windows Hello). */
export async function verifyUser(reason: string): Promise<AuthResult> {
  try {
    if (process.platform === 'darwin') {
      if (systemPreferences.canPromptTouchID()) {
        try {
          await systemPreferences.promptTouchID(reason)
          return 'ok'
        } catch {
          return 'denied'
        }
      }
      return await run('/usr/bin/osascript', ['-l', 'JavaScript', '-e', JXA, reason])
    }
    if (process.platform === 'win32') {
      const root = process.env.SystemRoot || 'C:\\Windows'
      const ps = `${root}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe`
      const encoded = Buffer.from(POWERSHELL, 'utf16le').toString('base64')
      return await run(ps, ['-NoProfile', '-NonInteractive', '-EncodedCommand', encoded], { SIGNET_AUTH_REASON: reason })
    }
    // Linux: no universal authentication prompt.
    return 'unavailable'
  } catch {
    return 'denied'
  }
}
