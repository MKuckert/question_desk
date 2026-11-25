// Renders the frontend (ui/) in an off-screen WKWebView with a stubbed Tauri bridge and writes
// a PNG of the page. It never reads the screen: it is for checking layout where window
// screenshots are not permitted (e.g. inside the nono sandbox). The native parts (title bar,
// traffic lights, sidebar vibrancy, menus, dialogs) are not in the picture; the sidebar gets
// a flat stand-in colour for its material.
//
//   swiftc -O -o /tmp/render tools/render-offscreen.swift
//   /tmp/render ui/ STATE.json OUT.png [light|dark] [WIDTHxHEIGHT] [JS to run before capture]
//
// STATE.json comes from `QD_DUMP_DIR=… cargo test --lib dump_states -- --ignored`.
import AppKit
import WebKit

let args = CommandLine.arguments
guard args.count >= 4 else {
    FileHandle.standardError.write("usage: render UI_DIR STATE.json OUT.png [light|dark] [WxH] [JS]\n".data(using: .utf8)!)
    exit(2)
}
let uiDir = URL(fileURLWithPath: args[1], isDirectory: true)
let state = try! String(contentsOfFile: args[2], encoding: .utf8)
let out = URL(fileURLWithPath: args[3])
let dark = args.count > 4 && args[4] == "dark"
let size = (args.count > 5 ? args[5] : "1100x760").split(separator: "x").map { CGFloat(Double($0)!) }
let extraJS = args.count > 6 ? args[6] : ""

let app = NSApplication.shared
app.setActivationPolicy(.prohibited)

let bridge = """
window.__TAURI__ = {
  core: { invoke: async (cmd, args) => {
    if (cmd === "state") return \(state);
    if (cmd === "settings") return { host: "127.0.0.1", port: "7462", token: "", path: "~/.config/question-desk/config.toml", defaultHost: "127.0.0.1", defaultPort: 7457 };
    if (cmd === "settings_warning") return null;
    return null;
  } },
  event: { listen: async () => () => {} },
};
"""
let sidebar = dark ? "#2a2a2e" : "#e9e7ea"
let css = "html[data-platform=macos] #sidebar { background: \(sidebar) !important; }"
let config = WKWebViewConfiguration()
config.userContentController.addUserScript(WKUserScript(source: bridge, injectionTime: .atDocumentStart, forMainFrameOnly: true))
config.userContentController.addUserScript(WKUserScript(
    source: "const s=document.createElement('style');s.textContent=`\(css)`;document.head.append(s);",
    injectionTime: .atDocumentEnd, forMainFrameOnly: true))

let frame = NSRect(x: 0, y: 0, width: size[0], height: size[1])
let window = NSWindow(contentRect: frame, styleMask: [.borderless], backing: .buffered, defer: false)
window.setFrameOrigin(NSPoint(x: -20000, y: -20000))
window.appearance = NSAppearance(named: dark ? .darkAqua : .aqua)
let web = WKWebView(frame: frame, configuration: config)
web.appearance = window.appearance
window.contentView = web
window.orderBack(nil)

final class Delegate: NSObject, WKNavigationDelegate {
    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.6) {
            webView.evaluateJavaScript(extraJS.isEmpty ? "0" : extraJS) { _, _ in
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) {
                    webView.takeSnapshot(with: nil) { image, error in
                        guard let image, let tiff = image.tiffRepresentation,
                              let png = NSBitmapImageRep(data: tiff)?.representation(using: .png, properties: [:])
                        else {
                            FileHandle.standardError.write("snapshot failed: \(String(describing: error))\n".data(using: .utf8)!)
                            exit(1)
                        }
                        try! png.write(to: out)
                        print(out.path)
                        exit(0)
                    }
                }
            }
        }
    }
}
let delegate = Delegate()
web.navigationDelegate = delegate
// Inlined rather than loaded from file: URLs, which need sandbox extensions nono refuses.
func read(_ name: String) -> String { try! String(contentsOf: uiDir.appendingPathComponent(name), encoding: .utf8) }
let html = read("index.html")
    .replacingOccurrences(of: "<link rel=\"stylesheet\" href=\"style.css\" />", with: "<style>\(read("style.css"))</style>")
    .replacingOccurrences(of: "<script src=\"app.js\" defer></script>", with: "")
    .replacingOccurrences(of: "</body>", with: "<script>\(read("app.js"))</script></body>")
web.loadHTMLString(html, baseURL: nil)
DispatchQueue.main.asyncAfter(deadline: .now() + 15) { FileHandle.standardError.write("timeout\n".data(using: .utf8)!); exit(1) }
app.run()
