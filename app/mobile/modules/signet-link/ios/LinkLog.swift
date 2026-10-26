import Foundation
import os

/// Link log: always in the system log (subsystem `dev.signet.link`), and also on standard output
/// when the app is launched with `SIGNET_LINK_DEBUG=1` (for example `xcrun devicectl device process launch --console`).
enum LinkLog {
  private static let logger = Logger(subsystem: "dev.signet.link", category: "link")
  private static let toStdout = ProcessInfo.processInfo.environment["SIGNET_LINK_DEBUG"] == "1"

  static func info(_ message: String) {
    logger.notice("\(message, privacy: .public)")
    if toStdout {
      print("[signet-link] \(message)")
      fflush(stdout)
    }
  }
}
