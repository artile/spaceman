// winwatch: logs, until SIGTERM, each change of the frontmost app and of every window that did not
// exist at start (frame, on screen, % visible on a display, z order). One line per change, flushed.
import AppKit

setvbuf(stdout, nil, _IOLBF, 0)
signal(SIGTERM) { _ in exit(0) }

func list() -> [[String: Any]] {
  (CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as? [[String: Any]]) ?? []
}
let initial = Set(list().compactMap { $0[kCGWindowNumber as String] as? Int })
// CG coordinates: origin at the top left of the main display, y down.
let mainHeight = NSScreen.screens.first?.frame.height ?? 0
let displays = NSScreen.screens.map {
  CGRect(x: $0.frame.minX, y: mainHeight - $0.frame.maxY, width: $0.frame.width, height: $0.frame.height)
}
var last: [Int: String] = [:]
var front: pid_t = -1
let t0 = Date()
print("0 START \(Int(t0.timeIntervalSince1970 * 1000))")
func ms() -> Int { Int(Date().timeIntervalSince(t0) * 1000) }

while true {
  RunLoop.current.run(until: Date().addingTimeInterval(0.02))
  let app = NSWorkspace.shared.frontmostApplication
  let f = app?.processIdentifier ?? 0
  if f != front {
    print("\(ms()) FRONT pid=\(f) \(app?.bundleIdentifier ?? "?")")
    front = f
  }
  let windows = list()
  let onscreen = windows.filter {
    ($0[kCGWindowLayer as String] as? Int) == 0 && (($0[kCGWindowIsOnscreen as String] as? Bool) ?? false)
  }
  for w in windows {
    guard let id = w[kCGWindowNumber as String] as? Int, !initial.contains(id),
      (w[kCGWindowLayer as String] as? Int) == 0,
      let b = w[kCGWindowBounds as String] as? [String: CGFloat]
    else { continue }
    let r = CGRect(x: b["X"]!, y: b["Y"]!, width: b["Width"]!, height: b["Height"]!)
    if r.width < 100 || r.height < 100 { continue }
    let on = (w[kCGWindowIsOnscreen as String] as? Bool) ?? false
    let area = displays.map { d -> CGFloat in
      let i = r.intersection(d)
      return i.isNull ? 0 : i.width * i.height
    }.reduce(0, +)
    let pct = on ? min(100, Int(area * 100 / (r.width * r.height))) : 0
    let z = onscreen.firstIndex { ($0[kCGWindowNumber as String] as? Int) == id } ?? -1
    let key = "\(Int(r.minX)),\(Int(r.minY)) \(Int(r.width))x\(Int(r.height)) on=\(on) visible=\(pct) z=\(z)"
    if last[id] != key {
      print("\(ms()) WIN \(id) pid=\(w[kCGWindowOwnerPID as String] ?? 0) \(key)")
      last[id] = key
    }
  }
}
