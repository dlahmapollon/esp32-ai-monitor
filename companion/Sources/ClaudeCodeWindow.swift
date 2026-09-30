/**
 * ClaudeCodeWindow.swift — eingebautes Fenster „Claude Code wartet“ (Issue #10).
 *
 * Claude Code meldet Hook-Ereignisse per HTTP an 127.0.0.1. Die App merkt sich,
 * welche Session auf den Nutzer wartet, und zeigt das als Szene im Fenster
 * `plugin:builtin.claude-code`; die Firmware behandelt es wie ein Plugin.
 *
 * Gegenstück zu companion-windows/crates/core/src/claude_code.rs. Zustände,
 * Texte und Szenenaufbau müssen dort und hier gleich bleiben. Das Eintragen
 * der Hooks in ~/.claude/settings.json läuft über den Rust-Helper.
 */

import AppKit
import Foundation
import Network
import Security

final class ClaudeCodeWindow {
    static let shared = ClaudeCodeWindow()
    static let viewID = "builtin.claude-code"
    static let view = DisplayPlugins.prefix + viewID
    static let label = "Claude Code"
    static let port: UInt16 = 47651
    static let path = "/claude-code"
    static let tokenHeader = "x-aimonitor-token"
    static let sessionTTL: TimeInterval = 12 * 3600
    static let maxBodyBytes = 256 * 1024
    private static let maxSessions = 32
    private static let maxRows = 4
    private static let maxProjectChars = 28

    enum Waiting: Int, Comparable {
        case permission = 0, input, done
        static func < (a: Waiting, b: Waiting) -> Bool { a.rawValue < b.rawValue }
    }

    struct Session {
        let id: String
        var project: String
        var waiting: Waiting?
        var since: Date
        var lastEvent: Date
    }

    enum ListenerState: Equatable {
        case starting, running, failed(String)
    }

    enum Setup { case ready, hooksMissing, listenerFailed }

    private(set) var sessions: [String: Session] = [:]
    private(set) var listenerState: ListenerState = .starting
    /// "missing" | "installed" | "outdated"; nil, solange unbekannt.
    private(set) var hookStatus: String?
    /// Letztes gültiges Hook-Ereignis, egal aus welcher Einstellungsdatei.
    private var lastEventAt: Date?
    var onChange: (() -> Void)?

    let token: String
    private var listener: NWListener?
    private var timer: Timer?
    private let queue = DispatchQueue(label: "de.aimonitor.claude-code")

    static var settingsURL: URL {
        FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".claude/settings.json")
    }

    private init() {
        let defaults = UserDefaults.standard
        if let stored = defaults.string(forKey: "claudeCodeToken"), stored.count >= 16 {
            token = stored
        } else {
            var bytes = [UInt8](repeating: 0, count: 16)
            if SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes) != errSecSuccess {
                bytes = (0..<16).map { _ in UInt8.random(in: 0...255) }
            }
            token = bytes.map { String(format: "%02x", $0) }.joined()
            defaults.set(token, forKey: "claudeCodeToken")
        }
    }

    // MARK: Start und Empfang

    func start() {
        startListener()
        refreshHookStatus()
        // Einmal pro Minute: Wartezeiten weiterzählen, verlassene Sessions
        // aufräumen, eine von Hand geänderte settings.json bemerken.
        timer = Timer.scheduledTimer(withTimeInterval: 60, repeats: true) { [weak self] _ in
            guard let self = self else { return }
            self.refreshHookStatus()
            if self.tick(now: Date()) { self.onChange?() }
        }
    }

    private func startListener() {
        do {
            let parameters = NWParameters.tcp
            parameters.acceptLocalOnly = true
            parameters.requiredLocalEndpoint = .hostPort(host: "127.0.0.1",
                                                         port: NWEndpoint.Port(rawValue: Self.port)!)
            let listener = try NWListener(using: parameters)
            listener.newConnectionHandler = { [weak self] connection in self?.accept(connection) }
            listener.stateUpdateHandler = { [weak self] state in
                DispatchQueue.main.async {
                    switch state {
                    case .ready: self?.setListener(.running)
                    case .failed(let error): self?.setListener(.failed(error.localizedDescription))
                    default: break
                    }
                }
            }
            listener.start(queue: queue)
            self.listener = listener
        } catch {
            setListener(.failed(error.localizedDescription))
        }
    }

    private func setListener(_ state: ListenerState) {
        guard listenerState != state else { return }
        listenerState = state
        if case .failed(let message) = state {
            NSLog("[ClaudeCode] Port %d nicht verfügbar: %@", Int(Self.port), message)
        }
        onChange?()
    }

    private func accept(_ connection: NWConnection) {
        connection.start(queue: queue)
        // Hooks schicken kleine Anfragen; hängende Verbindungen nicht halten.
        queue.asyncAfter(deadline: .now() + 2) { connection.cancel() }
        receive(connection, buffer: Data())
    }

    private func receive(_ connection: NWConnection, buffer: Data) {
        connection.receive(minimumIncompleteLength: 1, maximumLength: 65536) { [weak self] data, _, isComplete, error in
            guard let self = self else { return }
            var buffer = buffer
            if let data { buffer.append(data) }
            switch Self.parse(buffer) {
            case .incomplete:
                if isComplete || error != nil { connection.cancel(); return }
                self.receive(connection, buffer: buffer)
            case .invalid:
                self.respond(connection, status: 400)
            case .complete(let method, let path, let token, let body):
                var status = Self.check(method: method, path: path, token: token, expected: self.token)
                if status == 200 {
                    if let event = try? JSONSerialization.jsonObject(with: body) as? [String: Any] {
                        DispatchQueue.main.async { self.apply(event, now: Date()) }
                    } else {
                        status = 400
                    }
                }
                self.respond(connection, status: status)
            }
        }
    }

    private func respond(_ connection: NWConnection, status: Int) {
        let reason = [200: "OK", 400: "Bad Request", 401: "Unauthorized",
                      404: "Not Found", 405: "Method Not Allowed"][status] ?? "Error"
        // Leerer Body: für Claude Code keine Entscheidung, normaler Ablauf.
        let text = "HTTP/1.1 \(status) \(reason)\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        connection.send(content: Data(text.utf8), completion: .contentProcessed { _ in connection.cancel() })
    }

    enum Request {
        case incomplete, invalid
        case complete(method: String, path: String, token: String?, body: Data)
    }

    static func parse(_ buffer: Data) -> Request {
        let bytes = [UInt8](buffer)
        guard let end = (0..<max(0, bytes.count - 3)).first(where: {
            bytes[$0] == 13 && bytes[$0 + 1] == 10 && bytes[$0 + 2] == 13 && bytes[$0 + 3] == 10
        }) else {
            return bytes.count > 16 * 1024 ? .invalid : .incomplete
        }
        guard let head = String(bytes: bytes[..<end], encoding: .utf8) else { return .invalid }
        var lines = head.components(separatedBy: "\r\n")
        let first = lines.removeFirst().split(separator: " ", omittingEmptySubsequences: false)
        guard first.count >= 2 else { return .invalid }
        var length = 0
        var token: String?
        for line in lines {
            guard let colon = line.firstIndex(of: ":") else { return .invalid }
            let name = line[..<colon].lowercased()
            let value = line[line.index(after: colon)...].trimmingCharacters(in: .whitespaces)
            switch name {
            case "content-length":
                guard let n = Int(value), n >= 0, n <= maxBodyBytes else { return .invalid }
                length = n
            case tokenHeader:
                token = value
            case "transfer-encoding":
                return .invalid
            default:
                break
            }
        }
        let start = end + 4
        guard bytes.count >= start + length else { return .incomplete }
        let path = String(first[1].split(separator: "?", maxSplits: 1, omittingEmptySubsequences: false)[0])
        return .complete(method: String(first[0]), path: path, token: token,
                         body: Data(bytes[start..<(start + length)]))
    }

    static func check(method: String, path: String, token: String?, expected: String) -> Int {
        guard path == Self.path else { return 404 }
        guard method == "POST" else { return 405 }
        guard let token, !expected.isEmpty, token.utf8.count == expected.utf8.count else { return 401 }
        let diff = zip(token.utf8, expected.utf8).reduce(UInt8(0)) { $0 | ($1.0 ^ $1.1) }
        return diff == 0 ? 200 : 401
    }

    // MARK: Sessions

    /// Ein Hook-Ereignis übernehmen (Hauptthread).
    func apply(_ event: [String: Any], now: Date) {
        guard let id = event["session_id"] as? String, !id.isEmpty, id.utf8.count <= 128 else { return }
        let receiving = receivingEvents(now: now)
        lastEventAt = now
        // Das erste Ereignis löst „Hooks fehlen“ ab, auch wenn es selbst nichts listet.
        if update(id: id, event: event, now: now) || !receiving { onChange?() }
    }

    /// `true`, wenn sich die Anzeige ändern kann.
    private func update(id: String, event: [String: Any], now: Date) -> Bool {
        let name = event["hook_event_name"] as? String ?? ""
        if name == "SessionEnd" {
            return sessions.removeValue(forKey: id)?.waiting != nil
        }
        let current = sessions[id]?.waiting
        let next: Waiting?
        switch name {
        case "PermissionRequest": next = .permission
        case "Stop": next = .done
        case "StopFailure": next = .input
        case "UserPromptSubmit", "PostToolUse", "PostToolUseFailure", "SessionStart": next = nil
        case "Notification":
            switch event["notification_type"] as? String {
            case "permission_prompt": next = .permission
            // Kommt eine Minute nach `Stop`; „fertig“ bleibt dann stehen.
            case "idle_prompt" where current == .done: next = current
            case "idle_prompt", "agent_needs_input", "elicitation_dialog", "elicitation_url_dialog": next = .input
            case "agent_completed": next = .done
            case "elicitation_complete", "elicitation_response": next = nil
            default: return false
            }
        default:
            return false
        }
        var session = sessions[id] ?? Session(id: id, project: "", waiting: nil, since: now, lastEvent: now)
        if let cwd = event["cwd"] as? String {
            let project = Self.projectName(cwd)
            if !project.isEmpty { session.project = project }
        }
        if session.waiting != next { session.since = now }
        let changed = session.waiting != next || next != nil
        session.waiting = next
        session.lastEvent = now
        sessions[id] = session
        prune(now: now)
        return changed
    }

    /// Kommen Hook-Ereignisse an? Das beweist, dass Hooks wirken, auch wenn sie
    /// nicht in `~/.claude/settings.json` stehen (z. B. auf Projektebene).
    func receivingEvents(now: Date) -> Bool {
        guard let last = lastEventAt else { return false }
        return now.timeIntervalSince(last) < Self.sessionTTL
    }

    /// Minutentakt: aufräumen; `true`, wenn die Szene neu gesendet werden soll.
    func tick(now: Date) -> Bool {
        prune(now: now)
        var expired = false
        if let last = lastEventAt, now.timeIntervalSince(last) >= Self.sessionTTL {
            lastEventAt = nil
            expired = true
        }
        return expired || !waiting(now: now).isEmpty
    }

    private func prune(now: Date) {
        sessions = sessions.filter { now.timeIntervalSince($0.value.lastEvent) < Self.sessionTTL }
        while sessions.count > Self.maxSessions,
              let oldest = sessions.values.min(by: { $0.lastEvent < $1.lastEvent }) {
            sessions.removeValue(forKey: oldest.id)
        }
    }

    /// Freigabe vor Eingabe vor fertig, darin die älteste zuerst.
    func waiting(now: Date) -> [Session] {
        sessions.values
            .filter { $0.waiting != nil && now.timeIntervalSince($0.lastEvent) < Self.sessionTTL }
            .sorted { a, b in
                if a.waiting != b.waiting { return a.waiting! < b.waiting! }
                if a.since != b.since { return a.since < b.since }
                return a.id < b.id
            }
    }

    static func projectName(_ cwd: String) -> String {
        let trimmed = cwd.trimmingCharacters(in: CharacterSet(charactersIn: "/\\"))
        let base = trimmed.split(whereSeparator: { $0 == "/" || $0 == "\\" }).last.map(String.init) ?? ""
        return ascii(base, maxChars: maxProjectChars)
    }

    /// Umlaute umschreiben, anderes Nicht-ASCII durch „?“ ersetzen, kürzen.
    static func ascii(_ text: String, maxChars: Int) -> String {
        let map: [Character: String] = ["ä": "ae", "ö": "oe", "ü": "ue", "Ä": "Ae", "Ö": "Oe", "Ü": "Ue", "ß": "ss"]
        var out = ""
        for c in text {
            if let replacement = map[c] {
                out += replacement
            } else if let scalar = c.unicodeScalars.first, c.unicodeScalars.count == 1,
                      scalar.value >= 0x20, scalar.value <= 0x7E {
                out.append(c)
            } else {
                out += "?"
            }
        }
        if out.count > maxChars { out = String(out.prefix(maxChars - 3)) + "..." }
        return out
    }

    // MARK: Szene

    var setup: Setup {
        if case .failed = listenerState { return .listenerFailed }
        return (hookStatus == "installed" || receivingEvents(now: Date())) ? .ready : .hooksMissing
    }

    static func text(_ key: String, _ language: String) -> String {
        let de: [String: String] = [
            "title": "Claude Code", "quiet": "Alles ruhig", "quiet.hint": "Keine Session wartet auf dich",
            "setup": "Hooks fehlen", "setup.hint": "In der App unter Plugins einrichten",
            "listener": "Empfang gestoert", "listener.hint": "Port belegt? Details in der App",
            "permission": "Wartet auf Freigabe", "input": "Wartet auf Eingabe", "done": "Fertig",
            "now": "gerade eben",
        ]
        let en: [String: String] = [
            "title": "Claude Code", "quiet": "All quiet", "quiet.hint": "No session is waiting for you",
            "setup": "Hooks missing", "setup.hint": "Set them up under Plugins in the app",
            "listener": "Listener failed", "listener.hint": "Port in use? Details in the app",
            "permission": "Needs approval", "input": "Needs input", "done": "Done",
            "now": "just now",
        ]
        return (language == "en" ? en : de)[key] ?? ""
    }

    static func age(_ seconds: TimeInterval, _ language: String) -> String {
        let s = max(0, Int(seconds))
        if s < 60 { return text("now", language) }
        if s < 3600 { return "\(s / 60) min" }
        if s < 86400 { return "\(s / 3600) h" }
        return "\(s / 86400) d"
    }

    /// Szene im Format der Plugin-Fenster; hält die Firmware-Grenzen ein
    /// (24 Knoten, 1536 Bytes, druckbares ASCII).
    func scene(layout: String, language: String, theme: String, now: Date = Date()) -> [String: Any] {
        Self.scene(sessions: waiting(now: now), setup: setup, now: now,
                   layout: layout, language: language, theme: theme)
    }

    /// Wie `claude_code::scene` im Rust-Core; `sessions` ist bereits sortiert.
    static func scene(sessions list: [Session], setup: Setup, now: Date,
                      layout: String, language: String, theme: String) -> [String: Any] {
        let light = theme == "light"
        let background = light ? 0xF5F7FA : 1580575
        let primary = light ? 0x17212F : 0xFFFFFF
        let secondary = light ? 0x45566A : 0xABABAB
        let divider = light ? 0xD4DDE7 : 3717119
        let colors: [Waiting: Int] = light
            ? [.permission: 0xC25E00, .input: 0x8A6D00, .done: 0x1E7F3A]
            : [.permission: 0xFF9F0A, .input: 0xFFD60A, .done: 0x30D158]
        let portrait = layout == "portrait"
        let titleFont = portrait ? 20 : 24
        let nameFont = portrait ? 16 : 20
        let statusFont = 14
        let t = { (key: String) in text(key, language) }

        func message(_ title: String, _ hint: String, _ color: Int) -> [[String: Any]] {
            [
                ["type": "text", "x": 50, "y": 60, "w": 900, "h": 120, "color": primary, "font": titleFont, "text": t("title")],
                ["type": "rect", "x": 50, "y": 200, "w": 900, "h": 4, "color": divider],
                ["type": "text", "x": 50, "y": 380, "w": 900, "h": 140, "color": color, "font": titleFont, "align": "center", "text": title],
                ["type": "text", "x": 50, "y": 560, "w": 900, "h": 140, "color": secondary, "font": statusFont, "align": "center", "text": hint],
            ]
        }

        var nodes: [[String: Any]]
        switch setup {
        case .listenerFailed:
            nodes = message(t("listener"), t("listener.hint"), colors[.permission]!)
        case .hooksMissing:
            nodes = message(t("setup"), t("setup.hint"), colors[.permission]!)
        case .ready where list.isEmpty:
            nodes = message(t("quiet"), t("quiet.hint"), colors[.done]!)
        case .ready:
            let heading: String
            if language == "en" {
                heading = list.count == 1 ? "1 session waiting" : "\(list.count) sessions waiting"
            } else {
                heading = list.count == 1 ? "1 Session wartet" : "\(list.count) Sessions warten"
            }
            nodes = [
                ["type": "text", "x": 50, "y": 40, "w": 900, "h": 130, "color": primary, "font": titleFont, "text": heading],
                ["type": "rect", "x": 50, "y": 185, "w": 900, "h": 4, "color": divider],
            ]
            for (row, session) in list.prefix(maxRows).enumerated() {
                let y = 215 + row * 170
                let state = session.waiting ?? .done
                let color = colors[state]!
                let key = state == .permission ? "permission" : state == .input ? "input" : "done"
                let name = session.project.isEmpty ? label : session.project
                let status = "\(t(key)), \(age(now.timeIntervalSince(session.since), language))"
                nodes.append(["type": "circle", "x": 50, "y": y + 25, "w": 40, "h": 40, "color": color])
                nodes.append(["type": "text", "x": 120, "y": y, "w": 830, "h": 90, "color": primary, "font": nameFont, "text": name])
                nodes.append(["type": "text", "x": 120, "y": y + 90, "w": 830, "h": 70, "color": color, "font": statusFont, "text": status])
            }
            if list.count > maxRows {
                let rest = list.count - maxRows
                nodes.append(["type": "text", "x": 50, "y": 900, "w": 900, "h": 80, "color": secondary, "font": statusFont,
                              "text": language == "en" ? "+\(rest) more" : "+\(rest) weitere"])
            }
        }
        return ["background": background, "nodes": nodes]
    }

    // MARK: Hooks in ~/.claude/settings.json

    private func hooksHelper(_ action: String) throws -> String {
        let response = try DisplayPlugins.helper(
            ["claude-hooks", action, Self.settingsURL.path, String(Self.port)],
            environment: ["AIMONITOR_CLAUDE_TOKEN": token])
        return response["status"] as? String ?? "missing"
    }

    func refreshHookStatus(completion: (() -> Void)? = nil) {
        DispatchQueue.global(qos: .utility).async {
            let status = try? self.hooksHelper("status")
            DispatchQueue.main.async {
                if self.hookStatus != status {
                    self.hookStatus = status
                    self.onChange?()
                }
                completion?()
            }
        }
    }

    /// Hooks eintragen oder entfernen; die Oberfläche fragt vorher nach.
    func setHooks(install: Bool, completion: @escaping (Error?) -> Void) {
        DispatchQueue.global(qos: .userInitiated).async {
            let result = Result { try self.hooksHelper(install ? "install" : "remove") }
            DispatchQueue.main.async {
                switch result {
                case .success(let status):
                    self.hookStatus = status
                    self.onChange?()
                    completion(nil)
                case .failure(let error):
                    completion(error)
                }
            }
        }
    }
}
