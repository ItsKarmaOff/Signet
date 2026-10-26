// Dev-only for now: scans BLE adverts through CoreBluetooth for the phone passkey ("hybrid") flow and prints them as JSON
// lines on stdout. Built on demand by src/main/web/hybrid/corebluetooth.ts; never shipped in the packaged app.
// Bluetooth permission: launched from a terminal, macOS attributes it to the responsible app, which needs
// NSBluetoothAlwaysUsageDescription in its own Info.plist (VS Code has it; Terminal.app and Warp do not, and the
// process is killed). The Info.plist next to this file is embedded in the binary for the cases where it is honoured.
import CoreBluetooth
import Foundation

setvbuf(stdout, nil, _IOLBF, 0)

func emit(_ object: [String: String]) {
  guard let data = try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]),
    let line = String(data: data, encoding: .utf8)
  else { return }
  print(line)
  fflush(stdout)
}

func note(_ message: String) {
  FileHandle.standardError.write(Data("cable-scan: \(message)\n".utf8))
}

final class Scanner: NSObject, CBCentralManagerDelegate {
  private var central: CBCentralManager!
  private let wanted: [(CBUUID, String)] = [(CBUUID(string: "FFF9"), "fff9"), (CBUUID(string: "FDE2"), "fde2")]

  override init() {
    super.init()
    central = CBCentralManager(delegate: self, queue: DispatchQueue.main)
  }

  func centralManagerDidUpdateState(_ central: CBCentralManager) {
    let name: String
    switch central.state {
    case .poweredOn: name = "poweredOn"
    case .poweredOff: name = "poweredOff"
    case .unauthorized: name = "unauthorized"
    case .unsupported: name = "unsupported"
    case .resetting: name = "resetting"
    default: name = "unknown"
    }
    emit(["type": "state", "state": name])
    if central.state == .poweredOn {
      // Service-data-only adverts may not match a UUID filter: scan everything and filter here.
      central.scanForPeripherals(withServices: nil, options: [CBCentralManagerScanOptionAllowDuplicatesKey: true])
    }
  }

  func centralManager(
    _ central: CBCentralManager, didDiscover peripheral: CBPeripheral, advertisementData: [String: Any], rssi RSSI: NSNumber
  ) {
    guard let serviceData = advertisementData[CBAdvertisementDataServiceDataKey] as? [CBUUID: Data] else { return }
    for (uuid, data) in serviceData {
      guard let entry = wanted.first(where: { $0.0 == uuid }) else { continue }
      emit(["type": "data", "uuid": entry.1, "data": data.map { String(format: "%02x", $0) }.joined()])
    }
  }
}

// The parent closing stdin (or dying) ends the process.
DispatchQueue.global().async {
  while FileHandle.standardInput.availableData.count > 0 {}
  exit(0)
}
signal(SIGTERM, SIG_IGN)
let term = DispatchSource.makeSignalSource(signal: SIGTERM, queue: DispatchQueue.main)
term.setEventHandler { exit(0) }
term.resume()

let scanner = Scanner()
dispatchMain()
