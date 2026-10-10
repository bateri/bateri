// A Swift application hosting bateri's terminal pane through bt_embed.h — nothing of bateri's
// application on the way, as a product that embeds the pane would have it.
//
// With no argument it is the check `make embed-swift` runs: the pane's shell, with the shell
// integration (`BT_EMBED_ZSH`), is given a variable and a directory and a command that writes both
// to a file and fails; its start and end must reach this host as events, with its exit code, and
// the host then types `exit` — the shell's exit must reach it too, and the file must hold what was
// given. Before that, the layout engine answers a drag and plans its landing. With `--interactive`
// it is a plain window with a shell, for a person to try.

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
    /// The finished command's exit code, as its event told it.
    var commandExit: Int32?
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
        if commandExit != 1 { failures.append("the command's exit came as \(String(describing: commandExit)), expected 1") }
        if let started = heard.firstIndex(of: BT_EVENT_COMMAND_STARTED),
           let finished = heard.firstIndex(of: BT_EVENT_COMMAND_FINISHED) {
            if started > finished { failures.append("the command ended before it started") }
        } else {
            failures.append("no command events")
        }
        if uuid.count != 36 || uuid != uuid.uppercased() { failures.append("uuid \"\(uuid)\"") }
        if failures.isEmpty {
            print("embed-swift: ok (layout, \(heard.count) events)")
            exit(0)
        }
        print("embed-swift: FAILED — " + failures.joined(separator: "; "))
        exit(1)
    }
}

let host = Host()

/// The layout engine asked the way a host asks it: a picture of a workspace window with two tabs,
/// a verdict under the pointer while pane 110 is carried over pane 101, the landing planned, and a
/// strip's New Tab. What does not hold comes back as a list.
func checkLayout() -> [String] {
    var failures: [String] = []
    guard let world = bt_world_new(1000) else { return ["no picture"] }
    defer { bt_world_free(world) }
    _ = bt_world_add_window(world, 1, true, false)
    let pair = bt_tree_split(BT_AXIS_HORIZONTAL, 0.5, bt_tree_leaf(100), bt_tree_leaf(101))
    _ = bt_world_add_tab(world, 1, 10, pair, 101, "build", true)
    bt_tree_free(pair)
    let lone = bt_tree_leaf(110)
    _ = bt_world_add_tab(world, 1, 11, lone, 110, nil, false)
    bt_tree_free(lone)
    _ = bt_world_set_area(world, 10, 0, 0, 800, 600, 2)
    for pane: UInt64 in [100, 101, 110] {
        _ = bt_world_set_minimum(world, pane, 100, 100)
    }

    let carried = bt_tree_leaf(110)
    defer { bt_tree_free(carried) }
    guard let verdict = bt_verdict_new(world, 10, carried, 700, 300) else { return ["no verdict"] }
    defer { bt_verdict_free(verdict) }
    if bt_verdict_kind(verdict) != BT_VERDICT_LANDS || bt_verdict_zone(verdict) != BT_ZONE_BESIDE {
        failures.append("verdict \(bt_verdict_kind(verdict))/\(bt_verdict_zone(verdict)), expected lands beside")
    }
    guard let move = bt_move_pane_to_tab_planned(110, 10, bt_verdict_tree(verdict)) else {
        return failures + ["no move"]
    }
    defer { bt_move_free(move) }
    var refusal: Int32 = -1
    guard let plan = bt_plan_new(world, move, &refusal) else { return failures + ["refused \(refusal)"] }
    defer { bt_plan_free(plan) }
    var kinds: [UInt32] = []
    for index in 0..<bt_plan_step_count(plan, BT_PART_MAIN) {
        kinds.append(bt_plan_step_kind(plan, BT_PART_MAIN, index))
    }
    // A host refuses a plan with a main step it does not know.
    if kinds.contains(where: { $0 < BT_STEP_RELEASE_PANE || $0 > BT_STEP_PULSE }) {
        failures.append("unknown main step in \(kinds)")
    }
    if let landed = kinds.firstIndex(of: BT_STEP_ADOPT_PANES) {
        let tree = bt_plan_step_tree(plan, BT_PART_MAIN, landed)
        if bt_tree_pane_count(tree) != 3 { failures.append("the landing holds \(bt_tree_pane_count(tree)) panes") }
    } else {
        failures.append("no ADOPT_PANES in \(kinds)")
    }
    if let record = bt_plan_take_undo(plan) {
        bt_record_free(record)
    } else {
        failures.append("no Undo Move record")
    }

    guard let strip = bt_strip_new() else { return failures + ["no strip"] }
    defer { bt_strip_free(strip) }
    _ = bt_strip_append(strip, 1)
    _ = bt_strip_append(strip, 2)
    _ = bt_strip_insert(strip, 3)
    var selected: UInt64 = 0
    if !bt_strip_selected(strip, &selected) || selected != 3 || bt_strip_tab_at(strip, 1) != 3 {
        failures.append("New Tab did not open right of the selected tab")
    }
    return failures
}

let handler: BtEventHandler = { context, event in
    let host = Unmanaged<Host>.fromOpaque(context!).takeUnretainedValue()
    let kind = bt_event_kind(event)
    host.heard.append(kind)
    switch kind {
    case BT_EVENT_TITLE:
        if let title = host.take(bt_pane_title(host.pane)) {
            NSApp.windows.first?.title = title
        }
    case BT_EVENT_COMMAND_FINISHED:
        var code: Int32 = 0
        host.commandExit = bt_event_exit_code(event, &code) ? code : nil
        if !interactive {
            // The command is done: the host ends the shell, typed as the user would.
            let exit = Array("exit\n".utf8)
            _ = bt_pane_write(host.pane, exit, exit.count)
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

if !interactive {
    let failures = checkLayout()
    if !failures.isEmpty {
        print("embed-swift: FAILED — layout: " + failures.joined(separator: "; "))
        exit(1)
    }
}

guard let config = bt_pane_config_new(1, "bt-embed sample") else {
    print("embed-swift: FAILED — no configuration")
    exit(1)
}
_ = bt_pane_config_set_event_handler(config, handler, Unmanaged.passUnretained(host).toOpaque())
if !interactive {
    _ = bt_pane_config_set_working_directory(config, host.directory.path)
    _ = bt_pane_config_add_env(config, "BT_EMBED_PROBE", host.probe)
    _ = bt_pane_config_add_env(config, "BT_EMBED_OUT", host.output.path)
    guard let scripts = ProcessInfo.processInfo.environment["BT_EMBED_ZSH"] else {
        print("embed-swift: FAILED — BT_EMBED_ZSH names no shell integration scripts")
        exit(1)
    }
    _ = bt_pane_config_set_zsh_scripts(config, scripts)
    _ = bt_pane_config_set_command(
        config, "printf '%s|%s' \"$BT_EMBED_PROBE\" \"$PWD\" > \"$BT_EMBED_OUT\"; false")
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
