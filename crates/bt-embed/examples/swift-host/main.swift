// A Swift application hosting bateri's terminal pane through bt_embed.h — nothing of bateri's
// application on the way, as a product that embeds the pane would have it.
//
// With no argument it is the check `make embed-swift` runs: the pane's shell is given a variable
// and a directory and a command that writes both to a file and exits; the shell's exit must reach
// this host as an event, and the file must hold what was given. With `--interactive` it is a plain
// window with a shell, for a person to try.

import AppKit

setvbuf(stdout, nil, _IOLBF, 0)

let interactive = CommandLine.arguments.contains("--interactive")

guard bt_embed_abi_version() == BT_EMBED_ABI_VERSION else {
    print("embed-swift: the library speaks version \(bt_embed_abi_version()), this host \(BT_EMBED_ABI_VERSION)")
    exit(1)
}

/// What the host keeps between events.
final class Host {
    var pane: OpaquePointer?
    var heard: [UInt32] = []
    let probe = "probe-\(getpid())"
    let directory: URL
    let output: URL

    init() {
        let base = URL(fileURLWithPath: NSTemporaryDirectory())
            .appendingPathComponent("bt-embed-\(getpid())", isDirectory: true)
        try? FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
        directory = base.resolvingSymlinksInPath()
        output = directory.appendingPathComponent("out")
    }

    /// A string this library handed over, freed here.
    func take(_ text: UnsafeMutablePointer<CChar>?) -> String? {
        guard let text else { return nil }
        defer { bt_string_free(text) }
        return String(cString: text)
    }

    /// The shell exited: the check is judged, the pane closed.
    func shellExited() {
        let uuid = take(bt_pane_uuid(pane)) ?? ""
        bt_pane_close(pane)
        pane = nil
        if interactive {
            NSApp.terminate(nil)
            return
        }
        let written = (try? String(contentsOf: output, encoding: .utf8)) ?? ""
        try? FileManager.default.removeItem(at: directory)
        let expected = "\(probe)|\(directory.path)"
        var failures: [String] = []
        if written != expected { failures.append("the shell wrote \"\(written)\", expected \"\(expected)\"") }
        if !heard.contains(BT_EVENT_TITLE) { failures.append("no title event") }
        if uuid.count != 36 || uuid != uuid.uppercased() { failures.append("uuid \"\(uuid)\"") }
        if failures.isEmpty {
            print("embed-swift: ok (\(heard.count) events)")
            exit(0)
        }
        print("embed-swift: FAILED — " + failures.joined(separator: "; "))
        exit(1)
    }
}

let host = Host()

let handler: BtEventHandler = { context, event in
    let host = Unmanaged<Host>.fromOpaque(context!).takeUnretainedValue()
    let kind = bt_event_kind(event)
    host.heard.append(kind)
    switch kind {
    case BT_EVENT_TITLE:
        if let title = host.take(bt_pane_title(host.pane)) {
            NSApp.windows.first?.title = title
        }
    case BT_EVENT_SHELL_EXITED:
        host.shellExited()
    case BT_EVENT_NOTIFY:
        let title = bt_event_text(event).map { String(cString: $0) } ?? ""
        print("embed-swift: notification: \(title)")
    default:
        break
    }
}

let app = NSApplication.shared
app.setActivationPolicy(interactive ? .regular : .accessory)

let window = NSWindow(
    contentRect: NSRect(x: 240, y: 240, width: 720, height: 440),
    styleMask: [.titled, .closable, .miniaturizable, .resizable],
    backing: .buffered,
    defer: false
)
window.title = "bt-embed"
window.isReleasedWhenClosed = false

guard let config = bt_pane_config_new(1, "bt-embed sample") else {
    print("embed-swift: FAILED — no configuration")
    exit(1)
}
_ = bt_pane_config_set_event_handler(config, handler, Unmanaged.passUnretained(host).toOpaque())
if !interactive {
    _ = bt_pane_config_set_working_directory(config, host.directory.path)
    _ = bt_pane_config_add_env(config, "BT_EMBED_PROBE", host.probe)
    _ = bt_pane_config_add_env(config, "BT_EMBED_OUT", host.output.path)
    _ = bt_pane_config_set_command(
        config, "printf '%s|%s' \"$BT_EMBED_PROBE\" \"$PWD\" > \"$BT_EMBED_OUT\"; exit")
}

guard let content = window.contentView,
      let pane = bt_pane_open(Unmanaged.passUnretained(content).toOpaque(), config)
else {
    print("embed-swift: FAILED — the pane did not open")
    exit(1)
}
host.pane = pane

if interactive {
    window.makeKeyAndOrderFront(nil)
    app.activate(ignoringOtherApps: true)
} else {
    window.orderFrontRegardless()
}
guard bt_pane_start(pane) else {
    print("embed-swift: FAILED — the shell did not start")
    exit(1)
}
_ = bt_pane_focus(pane)

if !interactive {
    DispatchQueue.main.asyncAfter(deadline: .now() + 20) {
        print("embed-swift: FAILED — no shell exit within 20 s; heard \(host.heard)")
        exit(1)
    }
}

app.run()
