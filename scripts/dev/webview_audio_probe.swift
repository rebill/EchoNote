// M0 Spike E: which compressed recording formats can the macOS WKWebView (Tauri's webview) play?
//
// Loads probe.html from the directory given as the first argument in an offscreen WKWebView,
// waits for the page to post its JSON result, prints it and exits. Playback is muted.
// Run through scripts/dev/webview_audio_probe.sh.

import AppKit
import WebKit

final class Probe: NSObject, WKScriptMessageHandler, WKNavigationDelegate {
    let webView: WKWebView
    let window: NSWindow

    init(directory: URL) {
        let config = WKWebViewConfiguration()
        config.mediaTypesRequiringUserActionForPlayback = []
        // A windowless or hidden web view is throttled and its timers stop.
        config.preferences.inactiveSchedulingPolicy = .none
        let controller = WKUserContentController()
        config.userContentController = controller
        webView = WKWebView(frame: NSRect(x: 0, y: 0, width: 400, height: 300), configuration: config)
        window = NSWindow(contentRect: NSRect(x: -10000, y: -10000, width: 400, height: 300),
                          styleMask: [.borderless], backing: .buffered, defer: false)
        window.contentView = webView
        window.orderFrontRegardless()
        super.init()
        controller.add(self, name: "result")
        controller.add(self, name: "log")
        webView.navigationDelegate = self
        webView.loadFileURL(directory.appendingPathComponent("probe.html"), allowingReadAccessTo: directory)
    }

    func userContentController(_ userContentController: WKUserContentController, didReceive message: WKScriptMessage) {
        guard let text = message.body as? String else { return }
        if message.name == "log" {
            FileHandle.standardError.write("probing \(text)\n".data(using: .utf8)!)
            return
        }
        print(text)
        exit(0)
    }

    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
        FileHandle.standardError.write("navigation failed: \(error)\n".data(using: .utf8)!)
        exit(1)
    }
}

let directory = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let probe = Probe(directory: directory)
DispatchQueue.main.asyncAfter(deadline: .now() + 150) {
    FileHandle.standardError.write("timed out\n".data(using: .utf8)!)
    exit(2)
}
app.run()
